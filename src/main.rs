use std::{borrow::Cow, collections::HashSet, env, fmt, io};

use chrono::{DateTime, Duration, Local, TimeZone};
use imap::ClientBuilder;
use utf7_imap::{decode_utf7_imap, encode_utf7_imap};

fn main() {
    // check args
    let args = env::args().collect::<Vec<String>>();
    let show_mailbox = args.len() > 1 && matches!(args[1].as_str(), "show-mailbox");
    if args.len() > 1 && !show_mailbox {
        print!(
            "Usage: auto-delete-email [command]
Commands:
  show-mailbox: show mailbox list
Environment variables:
  IMAP_SERVER: imap server address, e.g. imap.example.com or imap.example.com:993
  IMAP_USER: imap user name
  IMAP_PASS: imap password
  SELECT_MAILBOX: mailbox to select, default is INBOX
  TRASH_MAILBOX: mailbox to move deleted email to, default is Trash
  SEEN_BEFORE: delete seen email before N days, default is -1 (disabled)
  UNSEEN_BEFORE: delete unseen email before N days, default is -1 (disabled)
  KEEP: keep deleted email for N days before remove, default is 30
"
        );
        return;
    }

    if !show_mailbox {
        print!("Starting...\n");
    }
    let imap_server = env::var("IMAP_SERVER").expect("IMAP_SERVER not set");
    let user = env::var("IMAP_USER").expect("IMAP_USER not set");
    let passwd = env::var("IMAP_PASS").expect("IMAP_PASS not set");

    let (domain, port) = if imap_server.contains(":") {
        let mut iter = imap_server.split(':');
        let domain = iter.next().unwrap();
        let port = iter.next().unwrap().parse::<u16>().unwrap();
        (domain, port)
    } else {
        (imap_server.as_str(), 993)
    };

    let client = ClientBuilder::new(domain, port).connect().unwrap();

    let sess = client.login(user, passwd);

    let mut sess = match sess {
        Ok(sess) => sess,
        Err(e) => {
            panic!("login failed, error: {}", e.0);
        }
    };

    if show_mailbox {
        print!("Mailboxes:\n");
        for mailbox in sess.list(None, Some("*")).unwrap().iter() {
            print!("  {}\n", decode_utf7_imap(mailbox.name().to_string()));
        }
        return;
    }

    let select_mailbox = env::var("SELECT_MAILBOX").unwrap_or("INBOX".to_owned());
    print!("Login success.\nSelecting mailbox: {}\n", select_mailbox);
    let select_mailbox = encode_utf7_imap(select_mailbox);
    let trash_mailbox = env::var("TRASH_MAILBOX").unwrap_or("Trash".to_owned());
    let trash_mailbox = encode_utf7_imap(trash_mailbox);

    let mut sess = ImapSess::new(sess, Cow::Owned(select_mailbox), Cow::Owned(trash_mailbox));

    let now = Local::now();
    let before = env::var("SEEN_BEFORE")
        .unwrap_or("-1".to_owned())
        .parse()
        .expect("SEEN_BEFORE not a number");
    if before >= 0 {
        let before = now - Duration::days(before);
        let result = sess.delete_email(&before, true);
        if let Err(e) = result {
            print!("Delete seen before email failed, err: {}\n", e);
        }
    }

    let before = env::var("UNSEEN_BEFORE")
        .unwrap_or("-1".to_owned())
        .parse()
        .expect("UNSEEN_BEFORE not a number");
    if before >= 0 {
        let before = now - Duration::days(before);
        let result = sess.delete_email(&&before, false);
        if let Err(e) = result {
            print!("Delete unseen before email failed, err: {}\n", e);
        }
    }

    let keep = env::var("KEEP")
        .unwrap_or("30".to_owned())
        .parse()
        .expect("KEEP not a number");
    let before = now - Duration::days(keep);
    let result = sess.remove_deleted_email(&before);
    if let Err(e) = result {
        print!("Remove deleted email failed, err: {}\n", e);
    }

    print!("Done.\n");
}

struct ImapSess<'a, T: io::Write + io::Read> {
    sess: imap::Session<T>,
    support_move: Option<bool>,
    select_mailbox: Cow<'a, str>,
    trash_mailbox: Cow<'a, str>,
}

fn format_date<TZ: TimeZone>(date: &DateTime<TZ>) -> String
where
    TZ::Offset: fmt::Display,
{
    date.format("%d-%b-%Y").to_string()
}

impl<'a, T: io::Write + io::Read> ImapSess<'a, T> {
    fn new(
        sess: imap::Session<T>,
        select_mailbox: Cow<'a, str>,
        trash_mailbox: Cow<'a, str>,
    ) -> Self {
        Self {
            sess,
            support_move: None,
            select_mailbox,
            trash_mailbox,
        }
    }
    fn delete_email<TZ: TimeZone>(
        &mut self,
        date: &DateTime<TZ>,
        seen: bool,
    ) -> Result<(), AnyError>
    where
        TZ::Offset: fmt::Display,
    {
        if self.sess.select(self.select_mailbox.as_ref())?.exists == 0 {
            return Ok(());
        }
        let query = format!(
            "BEFORE {} {}",
            format_date(date),
            if seen { "SEEN" } else { "UNSEEN" }
        );
        let uids = self.sess.uid_search(query)?;
        if uids.is_empty() {
            return Ok(());
        }
        if let None = self.support_move {
            let capabilities = self.sess.capabilities()?;
            self.support_move = Some(capabilities.has_str("MOVE"));
        }
        let uid_set = Self::get_id_str(uids);
        let mailbox_name = &self.trash_mailbox;
        if self.support_move.unwrap() {
            self.sess.uid_mv(&uid_set, mailbox_name)?;
        } else {
            // fallback to copy and delete
            self.sess.uid_copy(&uid_set, mailbox_name)?;
            self.sess.uid_store(&uid_set, "+FLAGS (\\Deleted)")?;
            self.sess.expunge().map(|_| ())?;
        }
        Ok(())
    }

    fn remove_deleted_email<TZ: TimeZone>(&mut self, before: &DateTime<TZ>) -> Result<(), AnyError>
    where
        TZ::Offset: fmt::Display,
    {
        // search in Trash mailbox
        if self.sess.select(self.trash_mailbox.as_ref())?.exists == 0 {
            return Ok(());
        }

        let uids = self
            .sess
            .uid_search(format!("BEFORE {}", format_date(before)))?;
        if uids.is_empty() {
            return Ok(());
        }
        let uid_set = Self::get_id_str(uids);
        let fetches = self.sess.uid_fetch(&uid_set, "(INTERNALDATE)")?;
        let mut delete_uids = HashSet::with_capacity(fetches.len());
        for fetch in fetches.iter() {
            if let Some(uid) = fetch.uid {
                if let Some(date) = fetch.internal_date() {
                    if date < *before {
                        delete_uids.insert(uid);
                    }
                }
            }
        }
        let uid_set = Self::get_id_str(delete_uids);
        self.sess.uid_store(&uid_set, "+FLAGS (\\Deleted)")?;
        self.sess.expunge().map(|_| ())?;
        Ok(())
    }

    fn get_id_str(ids: HashSet<u32>) -> String {
        if ids.len() == 0 {
            return "".to_owned();
        }
        let mut ids = Vec::from_iter(ids);
        ids.sort();
        let mut id_builder = Vec::new();
        let mut before = ids[0];
        let mut start = before;
        let mut push = |start, before| {
            if start == before {
                id_builder.push(format!("{}", before));
            } else {
                id_builder.push(format!("{}:{}", start, before));
            }
        };
        for &id in ids[1..].iter() {
            if id - 1 != before {
                push(start, before);
                start = id;
            }
            before = id;
        }
        push(start, before);
        id_builder.join(",")
    }
}

#[derive(Debug)]
enum AnyError {
    Imap(imap::Error),
    Other(String),
}

impl From<imap::Error> for AnyError {
    fn from(e: imap::Error) -> Self {
        AnyError::Imap(e)
    }
}

impl From<String> for AnyError {
    fn from(s: String) -> Self {
        AnyError::Other(s)
    }
}

impl fmt::Display for AnyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnyError::Imap(e) => write!(f, "Imap error: {}", e),
            AnyError::Other(s) => write!(f, "{}", s),
        }
    }
}
