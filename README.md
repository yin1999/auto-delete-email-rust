# auto delete email

This tool is for auto delete email with imap protocol.

## Set up

To work with this tool, you need to set up environment variables.

```ini
IMAP_SERVER=imap.example.com:993
IMAP_USER=user@example.com
IMAP_PASS=password

# optional
SEEN_BEFORE=7 # delete email that has been seen before 7 days
UNSEEN_BEFORE=15 # delete email that has been unseen before 15 days
```

### service

Recommand to use `systemd` to run this tool.

#### service file

```ini
[Unit]
Description=auto delete email
After=network-online.target

[Service]
DynamicUser=yes
EnvironmentFile=/path/to/auto-delete-email/env
ExecStart=/path/to/auto-delete-email/main
Type=simple

[Install]
WantedBy=multi-user.target
```

#### timer file

Run on 4:10:01 every day.

```ini
[Unit]
Description=timer for auto delete email

[Timer]
OnCalendar=*-*-* 4:10:01

[Install]
WantedBy=timers.target
```
