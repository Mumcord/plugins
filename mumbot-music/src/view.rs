//! UI trees for the dialog and the settings form. Everything from the bot is already cleaned by
//! `api::clean`; the renderer shows it as plain text.

use mumcord_plugin_sdk::ui::{self, ButtonKind, Component, El, InputPurpose, TextInput};

use crate::{
    api::{Queue, Track},
    app::{Dialog, Mode, Player, SettingsEdit, Setup, Test},
    bots::BotConfig,
};

pub const NOT_IN_CHANNEL_TEXT: &str = "Mumbot is not in your channel";
pub const NO_CERTIFICATE_TEXT: &str =
    "This user has no certificate, so Mumcord can't recognise it as a bot again.";

/// The text-input's byte limit.
const INPUT_BYTES: u8 = 64;

pub fn dialog(dialog: &Dialog) -> El {
    let root = ui::column();
    match &dialog.mode {
        Mode::NoCertificate => root.child(ui::text(NO_CERTIFICATE_TEXT)),
        _ if !dialog.present => root
            .child(ui::heading(&dialog.name))
            .child(ui::text(NOT_IN_CHANNEL_TEXT))
            .child(ui::caption(
                "Its settings are kept. The controls come back when the bot returns.",
            )),
        Mode::Setup(setup) => setup_form(dialog, setup),
        Mode::Player(player) => player_view(dialog, player),
    }
}

fn setup_form(dialog: &Dialog, setup: &Setup) -> El {
    let editing = setup.saved.is_some();
    let heading = if editing {
        "Edit address / key"
    } else {
        "Set up this bot"
    };
    let key_label = if editing {
        "New API key (leave empty to keep the current one)"
    } else {
        "API key"
    };
    let mut form = ui::column()
        .child(ui::heading(heading))
        .child(ui::caption(&dialog.name))
        .child(address_input(
            "address",
            &setup.address,
            "192.168.1.20:8081 or https://bot.example.org",
        ))
        .child(ui::password(
            "key",
            key_label,
            &dialog.bot.secret(setup.slot),
        ));
    let status = match &setup.test {
        Test::Idle => None,
        Test::Running => Some(ui::progress("Testing the connection", None)),
        Test::Passed => Some(ui::text("Connection works").color("success")),
        Test::Failed(text) => Some(ui::text(text).color("danger")),
    };
    form = form.child(
        ui::button("test", "Test connection")
            .kind(ButtonKind::Secondary)
            .disabled(setup.test == Test::Running),
    );
    if let Some(status) = status {
        form = form.child(status);
    }
    let mut actions =
        ui::row().child(ui::button("save", "Save").disabled(setup.test != Test::Passed));
    if editing {
        actions = actions.child(ui::button("cancel", "Cancel").kind(ButtonKind::Secondary));
    }
    form.child(actions)
}

fn player_view(dialog: &Dialog, player: &Player) -> El {
    let off = player.offline.is_some();
    let mut root = ui::column().child(ui::caption(&dialog.name));
    if let Some(text) = &player.offline {
        root = root
            .child(ui::text(text).color("danger"))
            .child(ui::caption("The bot seems offline. Retrying…"));
    }
    let queue = player.queue.as_ref();
    root = root.child(now_playing(queue.and_then(|q| q.now_playing.as_ref())));
    root = root.child(
        ui::row()
            .child(ui::button("skip", "Skip").disabled(off))
            .child(
                ui::button("stop", "Stop")
                    .kind(ButtonKind::Danger)
                    .disabled(off),
            ),
    );
    let volume = queue.map_or(0, |q| q.volume);
    root = root.child(
        ui::slider("volume", "Volume", 0.0, 100.0, 5.0, f32::from(volume))
            .disabled(off || queue.is_none()),
    );
    root = root.child(queue_list(queue));
    root = root
        .child(ui::divider())
        .child(
            ui::text_input(
                "add-text",
                "Add a file name or URL",
                &player.add_text,
                INPUT_BYTES,
            )
            .disabled(off),
        )
        .child(ui::toggle("interrupt", "Play next, interrupt", player.interrupt).disabled(off))
        .child(
            ui::row()
                .child(ui::button("add", "Add track").disabled(off || player.add_text.is_empty()))
                .child(ui::file_picker("upload", "Upload an MP3", &[".mp3"]).disabled(off)),
        );
    if let Some(text) = &player.message {
        root = root.child(ui::text(text).color("warning"));
    }
    root = root.child(ui::divider());
    if player.confirm_remove {
        root.child(ui::text("Remove this bot and its API key?"))
            .child(
                ui::row()
                    .child(ui::button("remove-confirm", "Remove").kind(ButtonKind::Danger))
                    .child(ui::button("remove-cancel", "Cancel").kind(ButtonKind::Secondary)),
            )
    } else {
        root.child(
            ui::row()
                .child(ui::button("edit", "Edit address / key").kind(ButtonKind::Secondary))
                .child(ui::button("remove", "Remove this bot").kind(ButtonKind::Danger)),
        )
    }
}

fn now_playing(track: Option<&Track>) -> El {
    match track {
        None => ui::column().child(ui::text("Nothing is playing")),
        Some(track) => {
            let mut el = ui::column()
                .child(ui::caption("Now playing"))
                .child(ui::heading(&track.title));
            if let Some(secs) = track.duration_secs {
                el = el.child(ui::caption(&duration(secs)));
            }
            el
        }
    }
}

fn queue_list(queue: Option<&Queue>) -> El {
    let tracks = queue.map(|q| q.queue.as_slice()).unwrap_or_default();
    if tracks.is_empty() {
        return ui::caption("The queue is empty");
    }
    ui::list("Queue").children(
        tracks
            .iter()
            .take(50)
            .enumerate()
            .map(|(i, t)| ui::text(&format!("{}. {}", i + 1, t.title))),
    )
}

fn duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn address_input(action: &str, value: &str, placeholder: &str) -> El {
    El::new(Component::TextInput(TextInput {
        label: "Address".into(),
        value: value.to_owned(),
        placeholder: placeholder.to_owned(),
        max_bytes: INPUT_BYTES,
        purpose: InputPurpose::ServerAddress,
        action: action.to_owned(),
        disabled: false,
    }))
}

pub fn settings(saved: &[(String, BotConfig)], edit: Option<&SettingsEdit>) -> El {
    let mut root = ui::column().child(ui::heading("Saved bots"));
    if saved.is_empty() {
        return root.child(ui::caption(
            "No bots yet. Right-click a bot in your channel and choose Configure bot.",
        ));
    }
    let rows = saved.iter().map(|(key, config)| {
        ui::row()
            .child(
                ui::column()
                    .child(ui::text(&config.name))
                    .child(ui::caption(&config.address)),
            )
            .child(ui::button(&format!("edit:{key}"), "Edit").kind(ButtonKind::Secondary))
            .child(ui::button(&format!("remove:{key}"), "Remove").kind(ButtonKind::Danger))
    });
    root = root.child(ui::list("Saved bots").children(rows));
    if let Some(edit) = edit {
        root = root
            .child(ui::divider())
            .child(ui::text(&format!("Edit {}", edit.config.name)))
            .child(address_input("edit-address", &edit.address, ""))
            .child(ui::password(
                "edit-key",
                "New API key (leave empty to keep the current one)",
                &edit.bot.secret(edit.config.key.other()),
            ))
            .child(
                ui::row()
                    .child(ui::button("edit-save", "Save"))
                    .child(ui::button("edit-cancel", "Cancel").kind(ButtonKind::Secondary)),
            );
    }
    root
}
