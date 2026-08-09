# Deskfolk Single-File Prompt Pack

Each Markdown file inside `complete_prompts/` is fully self-contained.

## Use

For every generation:

1. Attach `deskfolk_yasser_modular_character_v1.json`.
2. Attach the canonical Yasser master reference sheet.
3. Open ONE file from `complete_prompts/`.
4. Copy the entire file into the image generator.
5. Generate.

You do not need to combine `global_complete.md` with a batch file anymore.

## Example

To generate the first right-hand sprite sheet, use:

```text
complete_prompts/modules/arms/right_hand/variants/variants_01.md
```

That one file already contains:

- character lock
- style lock
- JSON rules
- negative prompt
- dimensions
- pivots
- sheet layout
- variant order
- final execution instruction

## Sheet format

Most prompt files request:

```text
Cell size: 320×320 px
Grid: 4 columns × 2 rows
Final sheet: 1280×640 px
Maximum: 8 variants
```

Slice the generated image every 320 px after generation.
