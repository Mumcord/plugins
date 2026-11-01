# My plugin

A Mumcord plugin.

## Build

```sh
rustup target add wasm32-unknown-unknown
mumcord-plugin build            # writes dist/my-plugin-0.1.0.mcplugin
mumcord-plugin check dist/my-plugin-0.1.0.mcplugin
```

Load the folder in Mumcord under Settings → Developer (Developer mode) to try it.

## Licence

Add a `LICENSE` file with an OSI-approved licence before publishing.
