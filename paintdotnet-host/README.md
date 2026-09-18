# PaintFE Paint.NET legacy host

This optional, out-of-process host implements a small clean-room compatibility
profile for classic Paint.NET 3.5 CPU `PropertyBasedEffect` plugins. It does not
contain or redistribute Paint.NET binaries.

Publish a self-contained host next to PaintFE:

```bash
./publish.sh linux-x64
```

Supported RIDs are `win-x64`, `linux-x64`, `osx-x64`, and `osx-arm64`.
During development, set `PAINTFE_PDN_HOST` to the published executable.

The same host is also used for read-only `.pdn` project import. For plugin
effects, it executes untrusted third-party code and is crash isolation, not a
security sandbox. PaintFE requires explicit opt-in and per-plugin trust for
third-party plugin DLLs.
