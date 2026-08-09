# Sprite Sheet Batch Usage

Use the files in each module's `variants/` folder when you want several variants generated in one image.

For every generation:

1. Attach the canonical Yasser master reference sheet.
2. Attach `deskfolk_yasser_modular_character_v1.json`.
3. Paste `global/global_prompt.md`.
4. Paste one `variants/variants_XX.md` file.
5. Generate one sprite sheet.

## Standard sheet layout

Most batch files use:

```text
Cell size: 320×320 px
Columns: 4
Rows: 2
Sheet size: 1280×640 px
Maximum variants per sheet: 8
```

Each variant occupies one independent 320×320 transparent cell. The requested component must be placed at the same canonical coordinates inside every cell.

The model must not place several variants inside one 320×320 canvas. It must create a larger grid made from complete 320×320 cells.

After generation, slice the sheet every 320 px:

```text
cell_1 = x 0-319, y 0-319
cell_2 = x 320-639, y 0-319
cell_3 = x 640-959, y 0-319
cell_4 = x 960-1279, y 0-319

cell_5 = x 0-319, y 320-639
cell_6 = x 320-639, y 320-639
cell_7 = x 640-959, y 320-639
cell_8 = x 960-1279, y 320-639
```

Unused cells must remain fully transparent.
