use crate::utils::{create_client, handle_keys};
use clap::Args;
use nostr_sdk::prelude::*;
use std::{fs::OpenOptions, str::FromStr};
use tokio::signal::unix::{signal, SignalKind};

#[derive(Args)]
pub struct ListEventsSubCommand {
    /// Ids
    #[arg(short, long, action = clap::ArgAction::Append)]
    ids: Option<Vec<String>>,
    /// Authors
    #[arg(short, long, action = clap::ArgAction::Append)]
    authors: Option<Vec<String>>,
    /// Kinds
    #[arg(short, long, action = clap::ArgAction::Append)]
    kinds: Option<Vec<u64>>,
    /// p tag
    #[arg(short, long, action = clap::ArgAction::Append)]
    e: Option<Vec<String>>,
    /// p tag
    #[arg(short, long, action = clap::ArgAction::Append)]
    p: Option<Vec<String>>,
    /// Since
    #[arg(short, long, action = clap::ArgAction::Append)]
    since: Option<u64>,
    /// Until
    #[arg(short, long, action = clap::ArgAction::Append)]
    until: Option<u64>,
    /// Limit
    #[arg(short, long, action = clap::ArgAction::Append)]
    limit: Option<usize>,
    /// Output
    #[arg(short, long)]
    output: Option<String>,
    /// Alive
    #[arg(short, long, action = clap::ArgAction::Append)]
    alive: Option<bool>,
    // Print keys as hex
    #[arg(long, default_value = "false")]
    hex: bool,
}

pub fn list_events(
    private_key: Option<String>,
    relays: Vec<String>,
    difficulty_target: u8,
    sub_command_args: &ListEventsSubCommand,
) -> Result<()> {
    if relays.is_empty() {
        panic!("No relays specified, at least one relay is required!")
    }

    let keys = handle_keys(private_key, sub_command_args.hex)?;
    let client = create_client(&keys, relays, difficulty_target)?;

    let kinds: Option<Vec<Kind>> = sub_command_args
        .kinds
        .as_ref()
        .map(|kinds| kinds.iter().map(|k| Kind::from(*k)).collect());

    let events: Option<Vec<EventId>> = sub_command_args.e.as_ref().map(|events| {
        events
            .iter()
            .map(|e| EventId::from_hex(e.as_str()).expect("Invalid event id"))
            .collect()
    });

    let pubkeys: Option<Vec<XOnlyPublicKey>> = sub_command_args.p.as_ref().map(|pubs| {
        pubs.iter()
            .map(|p| XOnlyPublicKey::from_str(p.as_str()).expect("Invalid public key"))
            .collect()
    });
    let filters = vec![Filter {
        ids: sub_command_args.ids.clone(),
        authors: sub_command_args.authors.clone(),
        kinds,
        events,
        pubkeys,
        hashtags: None,
        references: None,
        search: None,
        since: sub_command_args.since.map(Timestamp::from),
        until: sub_command_args.until.map(Timestamp::from),
        limit: sub_command_args.limit,
        custom: Map::new(),
    }];
    if sub_command_args.alive.is_some() && sub_command_args.alive.unwrap() {
        nostr_sdk::block_on(handle_subscription(
            &client,
            filters,
            sub_command_args,
            keys,
        ))
        .ok();
    } else {
        handle_single_request(&client, filters, sub_command_args).ok();
    }

    Ok(())
}

fn handle_single_request(
    client: &blocking::Client,
    filters: Vec<Filter>,
    sub_command_args: &ListEventsSubCommand,
) -> Result<()> {
    let events: Vec<Event> = client.get_events_of(filters, None)?;

    if let Some(output) = &sub_command_args.output {
        let file = std::fs::File::create(output).unwrap();
        serde_json::to_writer_pretty(file, &events).unwrap();
        println!("Wrote {} event(s) to {}", events.len(), output);
    } else {
        for (i, event) in events.iter().enumerate() {
            if let Ok(e) = serde_json::to_string_pretty(event) {
                println!("{i}: {e:#}")
            }
        }
    }
    Ok(())
}

async fn handle_subscription(
    client: &blocking::Client,
    filters: Vec<Filter>,
    sub_command_args: &ListEventsSubCommand,
    keys: Keys,
) -> Result<()> {
    client.subscribe(filters);
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");
    let mut notifications = client.notifications();

    loop {
        tokio::select! {
            Ok(notification) = notifications.recv() => {
                handle_notification(notification, keys.clone(), sub_command_args)?;
            }
            _ = sigint.recv() => {
                client.clone().disconnect().expect("Failed disconnecting from the relays when shutting down");
                break;
            }
        }
    }

    Ok(())
}

fn handle_notification(
    notification: RelayPoolNotification,
    keys: Keys,
    sub_command_args: &ListEventsSubCommand,
) -> Result<()> {
    if let RelayPoolNotification::Event(_url, event) = notification {
        if event.kind == Kind::EncryptedDirectMessage {
            match decrypt(&keys.secret_key()?, &event.pubkey, &event.content) {
                Ok(msg) => {
                    if !sub_command_args.hex {
                        println!(
                            "Message from {} event id: {}, content: {}",
                            event.pubkey.to_bech32()?,
                            event.id.to_bech32()?,
                            msg
                        );
                    } else {
                        println!(
                            "Message from {} event id: {},  content: {}",
                            event.pubkey,
                            event.id.to_hex(),
                            msg
                        );
                    }
                }
                Err(e) => println!("Impossible to decrypt direct message: {e}"),
            }
        } else {
            if let Some(output) = &sub_command_args.output {
                let file = OpenOptions::new().create(true).append(true).open(output)?;
                serde_json::to_writer_pretty(file, &event).unwrap();
            }
            if let Ok(e) = serde_json::to_string_pretty(&event) {
                println!("{e:#}")
            }
        }
    }
    Ok(())
}
