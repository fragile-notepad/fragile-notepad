[Documentation](../README.md)

# Test and profile rendering

Use focused renderer checks, shared CSV traces, and the offscreen composition
probe to investigate rendering changes. This page also records the conditions
and limits of earlier measurements.

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

Run commands from the repository root. Start with the tests that cover the changed
renderer layer; run one resource-intensive command at a time. Use one Cargo build
job and one test thread for graphics checks.

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

The [composition probe](../../examples/profile_composition.rs) retains the real
runtime widget tree and layout between redraws. It rebuilds only after messages
or widget invalidation. With a prepared Vulkan runtime and cached dependencies,
run:

```sh
cargo run --locked --offline -j 1 --example profile_composition -- --compare --frames 48
```

The probe reports CPU update, recording, and submission time; Vulkan timestamp
median and p95; paint-cache counters; actual editor wheel messages; and fade
toggles. `--compare` runs each uncached and cached pair consecutively on one
device. `--uncached` disables both paint-cache budgets while preserving batching,
the bounded decorative field, and editor optimizations.

| Option | Purpose |
| --- | --- |
| `--case` | Select `app-repeat`, `app-about`, `vfx`, `app-scroll-reversal`, `app-fade`, or `all`. The default is `all`. |
| `--fixture` | Load a UTF-8 editor file. The default fixture is embedded from `src/app.rs`. |
| `--width`, `--height` | Set physical target dimensions. Defaults are 1536 by 1152 pixels. |
| `--scale` | Set logical-to-physical scale. The default is 1.5. |
| `--frames` | Set measured frames per case, from 1 to 240. The default is 32. |
| `--warmup` | Set unmeasured frames per case, from 1 to 32. The default is 8. |
| `--help` | Show all supported options. |
| `WGPU_ADAPTER_NAME` | Select a Vulkan adapter by a case-insensitive name substring. |

Redraws are forced at no more than 60 Hz. Timestamp readback waits happen outside
CPU timing. The probe excludes queued atlas uploads, swapchain presentation,
desktop composition, and background task execution. It measures neither OS GPU
utilization nor native window-drag latency.

Fade clocks use real elapsed time, so opacity progress and first-time shaping
can vary with machine load. The scroll-reversal case sends eight wheel lines per
frame and reverses direction every six frames. The fade case closes and reopens
About every ten measured frames. Run the probe serially with other
resource-intensive checks.

## Historical measurement evidence

### Initial investigation

The initial GPU investigation found that warm glyph and instance uploads were
already retained, but every requested redraw repainted the entire scene. A
captured static editor drew 1,854 glyph instances across 15 draws. About drew
2,124 across 24 draws, including the unchanged editor underneath it. These
captures made paint reuse and bounded damage the main targets.

### Paint-cache comparison on 2026-10-10

The check used an AMD Radeon 610M, Vulkan driver 25.10.36.11, a development build,
1536 by 1152 physical pixels at scale 1.5, and 48 measured frames after eight
warm-up frames. Its UTF-8 Japanese CSV fixture contained 600 lines and 47,689
bytes, with BLAKE3 hash
`740dda4cfdebba80e01dc2f0eeba8d2b22086206f6dea5b89a360be37147d5c7`.

Each cell is **uncached → cached**, in milliseconds. These compare paint caches
on the optimized code; they are not total before-and-after application timings.

| Workload | GPU median | CPU median | GPU p95 | CPU p95 |
| --- | ---: | ---: | ---: | ---: |
| Repeated editor paint | 1.082 → 0.911 | 2.864 → 0.456 | 1.494 → 1.329 | 3.387 → 0.860 |
| Settled About with live effects | 1.522 → 1.244 | 3.465 → 0.937 | 2.013 → 1.570 | 4.497 → 1.419 |
| Isolated decorative field | 0.489 → 0.480 | 0.610 → 0.667 | 0.776 → 0.807 | 1.039 → 1.030 |
| Rapid editor scroll reversals | 0.850 → 0.909 | 4.217 → 4.303 | 1.624 → 1.556 | 41.447 → 24.040 |
| Repeated dialog fade reversals | 1.573 → 1.347 | 3.859 → 4.305 | 2.173 → 2.545 | 52.734 → 57.814 |

The repeated editor reused all 48 completed frames without repainting. The live
effects case repainted 6,074,918 pixels instead of 84,934,656 full-frame pixels
(about 7.2%), with 240 child-surface hits and no offscreen recaptures.

Rapid scrolling sent 48 real editor wheel actions, reversing every six frames.
All 48 root frames rendered directly and no child surfaces were recaptured. The
fade case performed five close/open toggles and also avoided child recaptures.

Clock and load variation appeared across runs, including the isolated field
control where both modes take the same drawing path. The scroll median and fade
CPU and p95 results do not establish a speedup. First-time shaping and view
rebuilding still contribute long editor and transition frames. GPU scroll copying
and damage tiles remain unimplemented. The probe and pixel/event tests provide
further profiling points without establishing native presentation or drag
performance.

## Related documentation

- [Hybrid rendering](../architecture/rendering.md): policy, handoff, and rollback.
- [Vulkan rendering](../architecture/vulkan.md): resources and retained painting.
- [Testing](testing.md): application checks and platform requirements.
- [Packaging](packaging.md): runtime distribution.
- [Iced patch notes](../../vendor/iced/LOCAL_CHANGES.md): local framework changes.
