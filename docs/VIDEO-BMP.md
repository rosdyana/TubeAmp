# Classic video window geometry (VIDEO.BMP)

These are layout facts (coordinates only, no code) observed in Winamp's classic video-window drawing. Webamp does not implement this window. W and H are the window size in skin pixels.

- Frame insets: left 11, right 8, top 20, bottom 38. So the content is W-19 × H-58, at (11, 20).
- **Title bar** (20 px tall). The source y is 0 when focused and 21 when unfocused.
  - Left corner: src (0,y) 25×20, at x 0.
  - Filler: src (127,y) 25×20. There are `nt = (W-150)/25` fillers, split evenly on both sides of the title. When nt is odd, the left side gets a 12-px slice and the right a 13-px slice. Any remaining `(W-150)%25` px are a stretched filler before the right corner.
  - Title piece: src (26,y) 100×20, centred between the fillers.
  - Right corner: src (153,y) 25×20, at x W-25.
  - Close button: 9×9 at (W-11, 3). Normal art is part of the title bar; pressed is src (148,42).
- **Sides** run from y 20 to H-38. Left src (127,42) 11×29, right src (139,42) 8×29, tiled vertically, with the remainder stretched.
- **Bottom** (38 px tall, at y H-38):
  - Left piece: src (0,42) 125×38.
  - Fill: src (127,81) 25×38, tiled, with the remainder stretched.
  - Right piece: src (0,81) 125×38, at x W-125.
- **Bottom buttons:** 5 buttons, 15×18, at x = 9 + i·15, y = H-29. Normal art is part of the left bottom piece. Pressed is src (158 + i·15, 42). Classic order: fullscreen, 1×, 2×, TV/misc, options menu.
- **Info text:** drawn into rect x 92..W-25, y H-27..H-13 with the playlist font, using pledit Normal text over NormalBG. (Winamp uses playlist colour slots 4/5 here; map them to our closest pledit colours.)
- Default window size: 275×232.
