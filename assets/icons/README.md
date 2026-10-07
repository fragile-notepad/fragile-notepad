# Icons

SVGs live in `colored/`, `heroicons/`, and `bootstrap/`. Colored artwork uses a
22-unit canvas; monochrome controls use `currentColor`.

[Colored artwork](colored/LICENSE) is original project work, all rights reserved,
and excluded from the code's BSD license. Modified [Heroicons](heroicons/LICENSE)
and [Bootstrap Icons](bootstrap/LICENSE) retain MIT notices.
Redistribute [NOTICE.txt](NOTICE.txt) as `ICON-NOTICES.txt`.

Regenerate with the [asset scripts](../../README.md#build-from-source); outputs
are ignored by Git. The rasterizer supports paths, flat six-digit hex or
`currentColor` paint, and rounded caps/joins. Avoid transforms, inherited group
paint, and gradients. Use `fill-rule="evenodd"` for filled shapes with holes.
