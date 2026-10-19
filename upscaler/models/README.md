Put trained models here as executable `<name>.sh` scripts. The worker offers each one to Kellyfin as a model
called `<name>` and runs it as:

    <name>.sh INPUT OUTPUT TARGET_HEIGHT

It must write an MP4 (H.264/AAC) to OUTPUT whose short side is about TARGET_HEIGHT.
