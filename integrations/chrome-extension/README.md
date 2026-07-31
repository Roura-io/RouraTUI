# RouraTUI Browser Control

This Manifest V3 extension is the visible Chrome surface for RouraTUI. It inventories interactive elements, highlights targets, animates a coral cursor, and performs user-auditable clicks and text entry.

The extension communicates only with the registered native host `com.roura_io.rouratui`. Consequential-action approval is enforced by the native RouraTUI side before a command reaches Chrome.

During development, load this directory with `chrome://extensions` → **Developer mode** → **Load unpacked**. The production installer will register the extension and native-messaging host together.
