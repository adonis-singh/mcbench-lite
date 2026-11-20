These are `preferences.json` files as earlier builds of the window wrote them, each with a non-default value in every field it has.
`before-<field>.json` is the file of the builds before `<field>` was added (from that commit's `Preferences::to_text`); `today.json` is today's.
From the first release (2.0.0) on, each release adds the `preferences.json` its build writes here, named by its version (`2.0.0.json`).
`tests/preferences.rs` reads every file in this folder: each must load with nothing noted, so an update never costs anyone their preferences.
The `before-` files and `today.json` are also checked value by value; a field a file lacks must come out as the default's.
