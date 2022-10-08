use std::{collections::HashSet, env, io};

use chrono::{Duration, Local};
use imap;
use native_tls::TlsConnector;

fn main() {
    print!("Starting...\n");
    let imap_server = env::var("IMAP_SERVER").unwrap();
    let user = env::var("IMAP_USER").unwrap();
    let passwd = env::var("IMAP_PASS").unwrap();

    let domain = imap_server.split(":").next().unwrap();

    let tls = TlsConnector::builder().build().unwrap();

    let client = imap::connect(&imap_server, domain, &tls).unwrap();

    let mut sess = client.login(user, passwd).map_err(|e| e.0).unwrap();

    let result = sess.select("INBOX").unwrap();
    if result.exists == 0 {
        println!("Done.\n");
        return;
    }

    let mut sess = ImapSess::new(sess);

    let now = Local::now();
    let before = env::var("SEEN_BEFORE").unwrap_or("15".to_owned()).parse().unwrap();
    let before = now - Duration::days(before);
    sess.delete_email(format!("BEFORE {} SEEN", before.format("%d-%b-%Y").to_string())).unwrap();

    let before = env::var("UNSEEN_BEFORE").unwrap_or("30".to_owned()).parse().unwrap();
    let before = now - Duration::days(before);
    sess.delete_email(format!("BEFORE {}", before.format("%d-%b-%Y").to_string())).unwrap();

    println!("Done.\n");
}

struct ImapSess<T: io::Write + io::Read> {
    sess: imap::Session<T>,
    support_move: Option<bool>,
}

impl<T: io::Write + io::Read> ImapSess<T> {
    fn new(sess: imap::Session<T>) -> Self {
        Self {
            sess,
            support_move: None,
        }
    }
    fn delete_email(&mut self, query: String) -> Result<(), imap::Error> {
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
        } else { // fallback to copy and delete
            self.sess.uid_copy(&uid_set, mailbox_name)?;
            self.sess.uid_store(&uid_set, "+FLAGS (\\Deleted)")?;
            self.sess.expunge().map(|_| ())
        }
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
        id_builder.join(" ")
    }
}
