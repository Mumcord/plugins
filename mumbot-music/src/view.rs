//! UI trees for the dialog and the settings form. Everything from the bot is already cleaned by
//! `api::clean`; the renderer shows it as plain text.

use mumcord_plugin_sdk::ui::{
    self, ButtonKind, Component, El, InputPurpose, Justify, TextInput, Tone,
};

use crate::{
    api::{self, Queue, Track},
    app::{Dialog, Mode, Player, SettingsEdit, Setup, Test},
    bots::{self, BotConfig},
    text::{t, t_with},
};

/// The text-input's byte limit.
const INPUT_BYTES: u8 = 64;

pub fn dialog(dialog: &Dialog) -> El {
    let root = ui::column();
    match &dialog.mode {
        Mode::NoCertificate => root.child(ui::text(&t("no_certificate"))),
        _ if !dialog.present => root
            .child(ui::heading(&t("not_in_channel")))
            .child(ui::text(&t("not_in_channel_body")))
            .child(ui::caption(&t("bot_kept"))),
        Mode::Setup(setup) => setup_form(dialog, setup),
        Mode::Player(player) => player_view(player),
    }
}

fn setup_form(dialog: &Dialog, setup: &Setup) -> El {
    let editing = setup.saved.is_some();
    let key_label = if editing {
        t("key_label_edit")
    } else {
        t("key_label")
    };
    let (status, tone) = match &setup.test {
        Test::Passed => (t("status_test_passed"), Tone::Success),
        _ if editing => (t("status_connected"), Tone::Success),
        _ => (t("status_not_saved"), Tone::Danger),
    };
    // The key rejection belongs under the key field, every other failure under the address.
    let key_failed = matches!(&setup.test, Test::Failed(text) if *text == api::key_rejected_text());
    let mut address = address_input("address", &setup.address, &t("address_placeholder"));
    match &setup.test {
        Test::Failed(text) if !key_failed => address = address.error(text),
        Test::Passed => address = address.help(&t("address_ok")).help_tone(Tone::Success),
        _ if bots::base_url(&setup.address).is_some_and(|url| url.starts_with("http://")) => {
            address = address.help(&t("address_note_http"));
        }
        _ => {}
    }
    let mut key = ui::password("key", &key_label, &dialog.bot.secret(setup.slot));
    match &setup.test {
        Test::Failed(text) if key_failed => key = key.error(text),
        Test::Passed => key = key.help(&t("test_ok")).help_tone(Tone::Success),
        _ => {}
    }
    let mut form = ui::column()
        .child(
            ui::row()
                .justify(Justify::End)
                .child(ui::chip(&status).tone(tone)),
        )
        .child(ui::text(&t(if editing {
            "setup_intro_edit"
        } else {
            "setup_intro"
        })))
        .child(address)
        .child(key);
    if setup.test == Test::Running {
        form = form.child(ui::progress(&t("testing"), None));
    }
    let actions = ui::row()
        .justify(Justify::SpaceBetween)
        .child(
            ui::button("test", &t("test"))
                .kind(ButtonKind::Secondary)
                .disabled(setup.test == Test::Running),
        )
        .child(
            ui::row()
                .justify(Justify::End)
                .child(ui::button("cancel", &t("cancel")).kind(ButtonKind::Secondary))
                .child(
                    ui::button("save", &t("save"))
                        .kind(ButtonKind::Primary)
                        .disabled(setup.test != Test::Passed),
                ),
        );
    form.child(actions).child(ui::caption(&t("setup_footer")))
}

fn player_view(player: &Player) -> El {
    let off = player.offline.is_some();
    let queue = player.queue.as_ref();
    let mut left = ui::column();
    if let Some(text) = &player.offline {
        left = left
            .child(ui::text(text).color("danger"))
            .child(ui::caption(&t("offline_retry")));
    }
    left = left.child(now_playing(queue.and_then(|q| q.now_playing.as_ref())));
    left = left.child(
        ui::row()
            .child(ui::button("skip", &t("skip")).disabled(off))
            .child(
                ui::button("stop", &t("stop"))
                    .kind(ButtonKind::Danger)
                    .disabled(off),
            ),
    );
    let volume = queue.map_or(0, |q| q.volume);
    left = left.child(
        ui::slider("volume", &t("volume"), 0.0, 100.0, 5.0, f32::from(volume))
            .disabled(off || queue.is_none()),
    );
    left = left.child(queue_list(queue));
    left = left
        .child(ui::divider())
        .child(
            ui::text_input("add-text", &t("add_label"), &player.add_text, INPUT_BYTES)
                .disabled(off),
        )
        .child(ui::toggle("interrupt", &t("interrupt"), player.interrupt).disabled(off))
        .child(
            ui::row()
                .child(
                    ui::button("add", &t("add_track"))
                        .kind(ButtonKind::Primary)
                        .disabled(off || player.add_text.is_empty()),
                )
                .child(ui::file_picker("upload", &t("upload"), &[".mp3"]).disabled(off)),
        );
    if let Some(text) = &player.message {
        left = left.child(ui::text(text).color("warning"));
    }
    ui::row().child(left).child(bot_settings(player))
}

/// The "Bot settings" column next to the player.
fn bot_settings(player: &Player) -> El {
    let column = ui::column().child(ui::heading(&t("settings_title"))).child(
        ui::text_input(
            "address-shown",
            &t("address_label"),
            &player.config.address,
            INPUT_BYTES,
        )
        .disabled(true),
    );
    if player.confirm_remove {
        return column.child(ui::text(&t("remove_confirm"))).child(
            ui::row()
                .child(ui::button("remove-confirm", &t("remove")).kind(ButtonKind::Danger))
                .child(ui::button("remove-cancel", &t("cancel")).kind(ButtonKind::Secondary)),
        );
    }
    column
        .child(
            ui::row()
                .child(ui::button("edit", &t("edit_address_key")).kind(ButtonKind::Secondary))
                .child(ui::button("remove", &t("remove_bot")).kind(ButtonKind::Danger)),
        )
        .child(ui::caption(&t("remove_note")))
}

fn now_playing(track: Option<&Track>) -> El {
    match track {
        None => ui::column().child(ui::text(&t("nothing_playing"))),
        Some(track) => {
            let mut el = ui::column()
                .child(ui::caption(&t("now_playing")))
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
        return ui::caption(&t("queue_empty"));
    }
    ui::list(&t_with(
        "queue_count",
        &[("count", &tracks.len().to_string())],
    ))
    .children(
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
        label: t("address_label"),
        value: value.to_owned(),
        placeholder: placeholder.to_owned(),
        max_bytes: INPUT_BYTES,
        purpose: InputPurpose::ServerAddress,
        action: action.to_owned(),
        disabled: false,
        help: None,
        help_tone: None,
        error: None,
    }))
}

/// Where a saved bot is right now, shown as a status pill on its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotStatus {
    InChannel,
    NotInChannel,
    NotConnected,
}

pub fn settings(saved: &[(String, BotConfig, BotStatus)], edit: Option<&SettingsEdit>) -> El {
    let mut root = ui::column().child(ui::heading(&t("saved_bots")));
    if saved.is_empty() {
        return root.child(ui::caption(&t("no_bots")));
    }
    let rows = saved.iter().map(|(key, config, status)| {
        let chip = match status {
            BotStatus::InChannel => ui::chip(&t("status_in_channel")).tone(Tone::Success),
            BotStatus::NotInChannel => ui::chip(&t("status_not_in_channel")).tone(Tone::Warning),
            BotStatus::NotConnected => ui::chip(&t("status_not_connected")).tone(Tone::Neutral),
        };
        ui::row()
            .justify(Justify::SpaceBetween)
            .child(
                ui::row()
                    .child(ui::image("assets/icon-32.png", "", 32, 32))
                    .child(
                        ui::column()
                            .child(ui::text(&config.name))
                            .child(ui::caption(&config.address)),
                    )
                    .child(chip),
            )
            .child(
                ui::row()
                    .justify(Justify::End)
                    .child(
                        ui::button(&format!("edit:{key}"), &t("edit")).kind(ButtonKind::Secondary),
                    )
                    .child(
                        ui::button(&format!("remove:{key}"), &t("remove")).kind(ButtonKind::Danger),
                    ),
            )
    });
    root = root.child(ui::list(&t("saved_bots")).children(rows));
    if let Some(edit) = edit {
        root = root
            .child(ui::divider())
            .child(ui::text(&t_with(
                "edit_named",
                &[("name", &edit.config.name)],
            )))
            .child(address_input("edit-address", &edit.address, ""))
            .child(ui::password(
                "edit-key",
                &t("key_label_edit"),
                &edit.bot.secret(edit.config.key.other()),
            ))
            .child(
                ui::row()
                    .child(ui::button("edit-save", &t("save")))
                    .child(ui::button("edit-cancel", &t("cancel")).kind(ButtonKind::Secondary)),
            );
    }
    root
}
