# Testing

## 1. Unit tests on your own machine

The host interfaces only exist inside Mumcord, so a plugin crate cannot call them in a normal
`cargo test`. Keep the logic in plain Rust and put the host calls behind a small trait:

```rust
trait Host {
    fn get_count(&self) -> u32;
    fn set_count(&self, n: u32);
    fn send_wave(&self);
}

fn handle_wave(host: &impl Host) -> u32 {
    let n = host.get_count().saturating_add(1);
    host.set_count(n);
    host.send_wave();
    n
}
```

The real implementation calls `storage` and `peers`; tests use a fake that records calls. Test the
message encoding, limits, bad input from peers and every error path (`NotPermitted`,
`RateLimited`, `NoRecipients`). The official Poll plugin (`poll/` in this repository) is built this
way and is a good example. Compile the crate for the host target for tests
(`cargo test --lib`), and keep host-only code behind `#[cfg(target_arch = "wasm32")]`.

## 2. Check the package

```sh
mumcord-plugin build
mumcord-plugin check dist/my-plugin-0.1.0.mcplugin
```

`check` reports exactly what the client would reject: manifest errors, unknown or forbidden
permissions, missing English texts, oversized files, wasm that imports anything but
`mumcord:plugin/*`. Fix these first.

## 3. Load it in Mumcord

Switch on Developer mode in Mumcord, then load the project folder or package through the plugin
settings. Developer plugins:

- may use all permissions except the three official-only ones,
- are marked as developer plugins, and exchange messages only with other developer copies of the
  same id,
- show their log lines and call statistics in the plugin's diagnostics.

Test with two Mumcord clients (two copies on the same machine are fine) joined to the same local
test server. **Test only against a local Mumble server**, never a public one.

Things to try by hand:

- Install with only the required permissions, then turn each optional permission on and off.
- Alone in the channel (`NoRecipients` is normal), and with a second client.
- Light and dark theme, large text, reduced motion.
- Disable and enable the plugin; restart Mumcord; reconnect to the server.
- Update from a previous version (set `previous-version` handling).

## 4. A local test registry

To test the install and update flow, serve a registry on your machine:

```sh
mumcord-plugin keygen --unencrypted --out keys --name test     # test key only
mumcord-plugin serve-registry --packages dist --key keys/test.key
```

This serves your packages as the official registry on `http://127.0.0.1:<port>/v1/`, signed with the
test key. Point a development build of Mumcord at it with the registry mirror setting and
`MUMCORD_PLUGIN_REGISTRY_KEY=<public key line>`. Add `--repository <name>` to serve a third-party
repository instead (see [Third-party repositories](third-party-repositories.md)).

`serve-registry` can also produce what the client must refuse, which is useful to see how your
update flow behaves: `tampered-hash:<id>`, `lower-sequence`, `wrong-key`, `extra-permission:<id>`,
`revoke:<id>[@<ver>][:warn]` and `key-change[:rotate]`. Switch them on with `--scenario` or while
running with `POST /_ctl/scenario?set=a,b`, and publish new versions with `POST /_ctl/reload`. The
scenario table is in
[`plugin-registry-format.md`](https://github.com/Mumcord/mumcord/blob/main/docs/plugin-registry-format.md#local-test-registry).

Never use a real signing key with the test registry, and never commit any key.
