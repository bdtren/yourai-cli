# yourai

Build and run your own local AI from the terminal.

```bash
npm install -g yourai
yourai doctor      # check this computer
yourai init        # guided setup
cd my-ai && yourai build && yourai start
yourai chat
```

This package only contains a small launcher. The prebuilt `yourai` binary for your platform comes
from one of the `@yourai/cli-*` optional dependencies; nothing is compiled or downloaded at install time.
Models are downloaded later by `yourai build`, after you confirm the license.

Docs: https://yourai.bdtren.com
