# Finding: what `ratatui-image` 11.1.0 would bring in (2026-11-04)

## Question

The rebuild plan named `ratatui-image` for real images (step 5). Would it fit our design, where one server
encodes images for several attached terminals? Is its licence picture clean? The choice between it and our own
encoders depends on this.

## Method

On 2026-11-04 I read, through a web fetch:
- `https://docs.rs/crate/ratatui-image/11.1.0/source/Cargo.toml.orig` for features and dependencies
- `https://docs.rs/ratatui-image/11.1.0/ratatui_image/picker/struct.Picker.html` for how it detects a terminal

`crates.io` itself didn't resolve from the fetch tool.

## Results

```toml
[features]
default = ["image-defaults", "crossterm", "chafa-dyn"]
chafa-dyn = ["dep:pkg-config"]
chafa-static = ["dep:pkg-config"]

[dependencies]
image = { version = "^0.25.6", default-features = false, features = ["png"] }
icy_sixel = { version = "^0.5.0" }
base64-simd = { version = "0.8" }
flate2 = { version = "^1.0" }
rand = { version = "^0.8.5" }
ratatui = { version = "^0.30.1", default-features = false, features = [] }
self_cell = "1.2.2"
thiserror = { version = "1.0.59" }
```

`Picker` methods:
- `from_query_stdio()` queries the terminal on the process's own stdin and stdout.
- `from_fontsize(font_size)` is "Deprecated since 9.0.0", in favour of `from_query_stdio` or `halfblocks`.
- `set_protocol_type` forces a protocol.

None of the crate's packages were in the local cargo registry, so using it would need a `cargo fetch`.

## Conclusions

- **Default features link `libchafa` (LGPL)** through `chafa-dyn`. We would have to turn default features off.
- **Detection runs in the program's own process.** Our session server talks to several clients, each with
  its own terminal, so detection has to happen in each client. For the server side, the only constructor is
  the deprecated `from_fontsize`.
- **It would add six crates** we don't otherwise need.
- **Decision:** our own encoders (kitty, iTerm2, and sixel with a fixed colour cube) and client-side detection.
  See [build steps 4–5](../plans/2026-11-04-build-steps-4-5.md).
