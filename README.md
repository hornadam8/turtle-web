# The Web Frontend

Needs nightly (maybe not but that's what we're doing for now)

Override to be nightly swaggin's
```
# cd turtle_chat/web_client
rustup override set nightly
```

Also make sure you've got WASM build target installed
```
rustup target add wasm32-unknown-unknown
```

Also have `trunk`
```
cargo install trunk
```

## Running
Do this:
```
trunk serve --address 0.0.0.0
```

