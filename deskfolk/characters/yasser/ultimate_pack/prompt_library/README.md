# Deskfolk Modular Prompt Pack

This pack is designed for generating reusable modular pixel-art assets for the character **Yasser**.

Attach these two references whenever you generate an asset:

1. `deskfolk_yasser_modular_character_v1.json`
2. The canonical Yasser master reference sheet

Then paste:

1. `global/global_prompt.md`
2. The matching module file
3. One final line naming the requested variant

Example:

```text
Generate variant: Thumbs Up
```

All assets use a shared 320×320 transparent authoring canvas. Do not crop during generation. Trim later in your asset pipeline while preserving the original offset and pivot metadata.


## Simplified Workflow (Recommended)

For every generation:

1. Attach `master_reference.png`
2. Attach `deskfolk_yasser_modular_character_v1.json`
3. Paste `global_complete.md`
4. Paste ONE batch file from `modules/**/variants/`
5. Generate

Do not paste the older global files unless you want to customize them manually.
