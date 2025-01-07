use std::{collections::HashSet, env, fmt, io};

use chrono::{DateTime, Duration, Local, TimeZone};
use imap::ClientBuilder;

fn main() {
    print!("Starting...\n");
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

    let result = sess.select("INBOX").unwrap();
    if result.exists == 0 {
        print!("Done.\n");
        return;
    }

    let mut sess = ImapSess::new(sess);

    let now = Local::now();
    let before = env::var("SEEN_BEFORE")
        .unwrap_or("15".to_owned())
        .parse()
        .expect("SEEN_BEFORE not a number");
    let before = now - Duration::days(before);
    let result = sess.delete_email(&format_date(&before), true);
    if let Err(e) = result {
        print!("Delete seen before email failed, err: {}\n", e);
    }

    let before = env::var("UNSEEN_BEFORE")
        .unwrap_or("30".to_owned())
        .parse()
        .expect("UNSEEN_BEFORE not a number");
    let before = now - Duration::days(before);
    let result = sess.delete_email(&format_date(&before), false);
    if let Err(e) = result {
        print!("Delete unseen before email failed, err: {}\n", e);
    }

    let keep = env::var("KEEP")
        .unwrap_or("30".to_owned())
        .parse()
        .expect("KEEP not a number");
    let before = before - Duration::days(keep);
    let result = sess.remove_deleted_email(&format_date(&before));
    if let Err(e) = result {
        print!("Remove deleted email failed, err: {}\n", e);
    }

    print!("Done.\n");
}

struct ImapSess<T: io::Write + io::Read> {
    sess: imap::Session<T>,
    support_move: Option<bool>,
}

fn format_date<TZ: TimeZone>(date: &DateTime<TZ>) -> String
where
    TZ::Offset: fmt::Display,
{
    date.format("%d-%b-%Y").to_string()
}

impl<T: io::Write + io::Read> ImapSess<T> {
    fn new(sess: imap::Session<T>) -> Self {
        Self {
            sess,
            support_move: None,
        }
    }
    fn delete_email(&mut self, date: &str, seen: bool) -> Result<(), imap::Error> {
        let query = format!("BEFORE {} {}", date, if seen { "SEEN" } else { "UNSEEN" });
        let uids = self.sess.uid_search(query)?;
        if uids.is_empty() {
            return Ok(());
        }
        if let None = self.support_move {
            let capabilities = self.sess.capabilities()?;
            self.support_move = Some(capabilities.has_str("MOVE"));
        }
        let uid_set = Self::get_id_str(uids);
        let mailbox_name = "Trash";
        if self.support_move.unwrap() {
            self.sess.uid_mv(&uid_set, mailbox_name)
        } else {
            // fallback to copy and delete
            self.sess.uid_copy(&uid_set, mailbox_name)?;
            self.sess.uid_store(&uid_set, "+FLAGS (\\Deleted)")?;
            self.sess.expunge().map(|_| ())
        }
    }

    fn remove_deleted_email(&mut self, before: &str) -> Result<(), imap::Error> {
        // search in Trash mailbox
        if !self.sess.select("Trash")?.exists == 0 {
            return Ok(());
        }

        let uids = self.sess.uid_search(format!("BEFORE {}", before))?;
        if uids.is_empty() {
            return Ok(());
        }
        let uid_set = Self::get_id_str(uids);
        self.sess.uid_store(&uid_set, "+FLAGS (\\Deleted)")?;
        self.sess.expunge().map(|_| ())
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
