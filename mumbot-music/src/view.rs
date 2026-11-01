//! UI trees for the dialog and the settings form. Everything from the bot is already cleaned by
//! `api::clean`; the renderer shows it as plain text.

use mumcord_plugin_sdk::ui::{self, ButtonKind, Component, El, InputPurpose, TextInput};

use crate::{
    api::{Queue, Track},
    app::{Dialog, Mode, Player, SettingsEdit, Setup, Test},
    bots::BotConfig,
    text::{t, t_with},
};

/// The text-input's byte limit.
const INPUT_BYTES: u8 = 64;

pub fn dialog(dialog: &Dialog) -> El {
    let root = ui::column();
    match &dialog.mode {
        Mode::NoCertificate => root.child(ui::text(&t("no_certificate"))),
        _ if !dialog.present => root
            .child(ui::heading(&dialog.name))
            .child(ui::text(&t("not_in_channel")))
            .child(ui::caption(&t("bot_kept"))),
        Mode::Setup(setup) => setup_form(dialog, setup),
        Mode::Player(player) => player_view(dialog, player),
    }
}

fn setup_form(dialog: &Dialog, setup: &Setup) -> El {
    let editing = setup.saved.is_some();
    let heading = if editing {
        t("edit_address_key")
    } else {
        t("set_up_bot")
    };
    let key_label = if editing {
        t("key_label_edit")
    } else {
        t("key_label")
    };
    let mut form = ui::column()
        .child(ui::heading(&heading))
        .child(ui::caption(&dialog.name))
        .child(address_input(
            "address",
            &setup.address,
            &t("address_placeholder"),
        ))
        .child(ui::password(
            "key",
            &key_label,
            &dialog.bot.secret(setup.slot),
        ));
    let status = match &setup.test {
        Test::Idle => None,
        Test::Running => Some(ui::progress(&t("testing"), None)),
        Test::Passed => Some(ui::text(&t("test_ok")).color("success")),
        Test::Failed(text) => Some(ui::text(text).color("danger")),
    };
    form = form.child(
        ui::button("test", &t("test"))
            .kind(ButtonKind::Secondary)
            .disabled(setup.test == Test::Running),
    );
    if let Some(status) = status {
        form = form.child(status);
    }
    let mut actions =
        ui::row().child(ui::button("save", &t("save")).disabled(setup.test != Test::Passed));
    if editing {
        actions = actions.child(ui::button("cancel", &t("cancel")).kind(ButtonKind::Secondary));
    }
    form.child(actions)
}

fn player_view(dialog: &Dialog, player: &Player) -> El {
    let off = player.offline.is_some();
    let mut root = ui::column().child(ui::caption(&dialog.name));
    if let Some(text) = &player.offline {
        root = root
            .child(ui::text(text).color("danger"))
            .child(ui::caption(&t("offline_retry")));
    }
    let queue = player.queue.as_ref();
    root = root.child(now_playing(queue.and_then(|q| q.now_playing.as_ref())));
    root = root.child(
        ui::row()
            .child(ui::button("skip", &t("skip")).disabled(off))
            .child(
                ui::button("stop", &t("stop"))
                    .kind(ButtonKind::Danger)
                    .disabled(off),
            ),
    );
    let volume = queue.map_or(0, |q| q.volume);
    root = root.child(
        ui::slider("volume", &t("volume"), 0.0, 100.0, 5.0, f32::from(volume))
            .disabled(off || queue.is_none()),
    );
    root = root.child(queue_list(queue));
    root = root
        .child(ui::divider())
        .child(
            ui::text_input("add-text", &t("add_label"), &player.add_text, INPUT_BYTES)
                .disabled(off),
        )
        .child(ui::toggle("interrupt", &t("interrupt"), player.interrupt).disabled(off))
        .child(
            ui::row()
                .child(
                    ui::button("add", &t("add_track")).disabled(off || player.add_text.is_empty()),
                )
                .child(ui::file_picker("upload", &t("upload"), &[".mp3"]).disabled(off)),
        );
    if let Some(text) = &player.message {
        root = root.child(ui::text(text).color("warning"));
    }
    root = root.child(ui::divider());
    if player.confirm_remove {
        root.child(ui::text(&t("remove_confirm"))).child(
            ui::row()
                .child(ui::button("remove-confirm", &t("remove")).kind(ButtonKind::Danger))
                .child(ui::button("remove-cancel", &t("cancel")).kind(ButtonKind::Secondary)),
        )
    } else {
        root.child(
            ui::row()
                .child(ui::button("edit", &t("edit_address_key")).kind(ButtonKind::Secondary))
                .child(ui::button("remove", &t("remove_bot")).kind(ButtonKind::Danger)),
        )
    }
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
    ui::list(&t("queue")).children(
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
        label: t("address"),
        value: value.to_owned(),
        placeholder: placeholder.to_owned(),
        max_bytes: INPUT_BYTES,
        purpose: InputPurpose::ServerAddress,
        action: action.to_owned(),
        disabled: false,
    }))
}

pub fn settings(saved: &[(String, BotConfig)], edit: Option<&SettingsEdit>) -> El {
    let mut root = ui::column().child(ui::heading(&t("saved_bots")));
    if saved.is_empty() {
        return root.child(ui::caption(&t("no_bots")));
    }
    let rows = saved.iter().map(|(key, config)| {
        ui::row()
            .child(
                ui::column()
                    .child(ui::text(&config.name))
                    .child(ui::caption(&config.address)),
            )
            .child(ui::button(&format!("edit:{key}"), &t("edit")).kind(ButtonKind::Secondary))
            .child(ui::button(&format!("remove:{key}"), &t("remove")).kind(ButtonKind::Danger))
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
