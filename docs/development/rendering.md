[Documentation](../README.md)

# Test and profile rendering

Run commands from the repository root.

## Prepare a Vulkan runtime

GPU checks need the `hybrid-rendering` feature, which is enabled by default, and
an available Vulkan adapter. Prepare the platform runtime before running the
checks below.

| Platform | Runtime |
| --- | --- |
| Windows | The graphics driver's Vulkan runtime. CI uses SwiftShader through [`scripts/setup-ci-vulkan.ps1`](../../scripts/setup-ci-vulkan.ps1). |
| Linux | A Vulkan loader and driver. CI uses Lavapipe through [`scripts/setup-ci-vulkan.sh`](../../scripts/setup-ci-vulkan.sh). |
| macOS | Vulkan loader and MoltenVK. Release bundles include them through [`scripts/package-macos.sh`](../../scripts/package-macos.sh). |

On macOS, install the development runtime and source the helper in the same shell
that will run Cargo:

```sh
brew install molten-vk vulkan-loader vulkan-tools
source scripts/setup-macos-vulkan.sh
```

If you need the full repository check script, source `scripts/ci.sh` from that
same shell. A new system shell can lose `DYLD_LIBRARY_PATH` through SIP.

Keep `WGPU_BACKEND` unset or set to `vulkan`. `WGPU_POWER_PREF=low|high|none`
overrides the application's adapter preference. See
[runtime options](../reference/runtime-options.md) for backend controls.

## Check a rendering change

Start with the tests that cover the changed renderer layer; run one
resource-intensive command at a time. Use one Cargo build job and one test thread
for graphics checks.

Alongside the [application checks](testing.md), renderer changes use these library
checks:

```sh
cargo test --locked -j 1 -p iced_graphics --lib -- --test-threads=1
cargo test --locked -j 1 -p iced_tiny_skia --lib -- --test-threads=1
cargo test --locked -j 1 -p iced_winit -p iced_wgpu --lib -- --test-threads=1
```

Choose an application integration target for the behavior you changed:

| Test target | Coverage |
| --- | --- |
| `vulkan_composition` | Damage, frame reuse, presentation, invalidation, and direct-rendering policy. |
| `vulkan_raster_cache` | Retained child surfaces, clipping, transparency, ownership, and asynchronous images. |
| `vulkan_resource_reuse` | Atlas limits, buffer growth, shared pipelines, and image allocation. |
| `vulkan_transform_parameters` | Immediate parameters compared with uniform parameters at device limits. |

For example, check retained window frames with:

```sh
cargo test --locked -j 1 --test vulkan_composition -- --test-threads=1
```

Set `FRAGILE_REQUIRE_GPU_TESTS=1` to fail when no Vulkan adapter is available,
or `0` to allow skips. When unset, adapters are required in CI and optional
locally. Device creation and rendering failures always fail. Linux CI and
nightly builds use `1`; other hosted platforms use `0`.

## Capture a trace

1. Choose a fresh output directory for each capture.
2. Set `FRAGILE_PERF_TRACE=1`. Set `FRAGILE_PERF_TRACE_DIR` to the output directory
   if you do not want the default `target/perf`.
3. Launch the application and reproduce the interaction.
4. Inspect `fragile-perf.csv` in that directory.

Application and runtime events append to the same CSV. Rows contain
`timestamp_us`, `event`, `elapsed_us`, and `detail`. Tracing adds formatting and
I/O cost, so compare captures made under the same tracing conditions.

## Profile offscreen composition

The [profiling example](../../examples/profile_composition.rs) uses the application's
widget tree and layout, rebuilding after messages or widget invalidation. Prepare
a Vulkan runtime and build dependencies, then run:

```sh
cargo run --release --locked --offline -j 1 --example profile_composition -- --compare --frames 48
```

The output includes CPU update, recording, and submission times; GPU median and
p95 times; cache counters; wheel events; and fade toggles. `--compare` measures
each scene first with caching disabled, then enabled, on one device. `--uncached`
disables the child-surface and final-frame caches. Batching, the decorative
field's reduced resolution, and editor optimizations stay enabled in both modes.

| Option | Purpose |
| --- | --- |
| `--case` | Select `app-repeat`, `app-about`, `vfx`, `app-scroll-reversal`, `app-fade`, or `all`. The default is `all`. |
| `--fixture` | Load a UTF-8 editor file. The default fixture is embedded from `src/app.rs`. |
| `--width`, `--height` | Set physical target dimensions. Defaults are 1536 by 1152 pixels. |
| `--scale` | Set logical-to-physical scale. The default is 1.5. |
| `--antialiasing` | Select `off` or `msaa4`. Defaults to `msaa4`, matching the application's hardware renderer. |
| `--frames` | Set measured frames per case, from 1 to 240. The default is 32. |
| `--warmup` | Set unmeasured frames per case, from 1 to 32. The default is 8. |
| `--help` | Show all supported options. |
| `WGPU_ADAPTER_NAME` | Select a Vulkan adapter by a case-insensitive name substring. |

Redraws are forced at no more than 60 Hz. Timestamp readback waits happen outside
CPU timing. Measurements exclude queued atlas uploads, swapchain presentation,
desktop composition, and background task execution. It measures neither OS GPU
utilization nor native window-drag latency.

Fade clocks use real elapsed time, so opacity progress and first-time shaping
can vary with machine load. The scroll-reversal case sends eight wheel lines per
frame and reverses direction every six frames. The fade case closes and reopens
About every ten measured frames.
