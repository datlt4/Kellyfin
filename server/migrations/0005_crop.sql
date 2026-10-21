-- Black bars found by ffmpeg cropdetect: NULL = not checked yet, '' = none, else the picture as w:h:x:y.
ALTER TABLE items ADD COLUMN crop TEXT;
