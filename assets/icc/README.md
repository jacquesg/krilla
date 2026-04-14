`krilla-generic-cmyk-v2.icc` is a compact synthetic CMYK output profile used by
krilla's examples and tests.

It is intentionally small so that snapshot PDFs and example outputs stay easy to
inspect and share. It is a CMYK ICC v2.4 `prtr` profile with Lab PCS and linked
intent tags, generated with LittleCMS from compact synthetic `A2B`/`B2A` CLUTs.

It is not a press characterization profile and should not be used as a real
production printing condition. For production PDF/X output, callers should
provide the actual press/output ICC profile that matches their workflow.
