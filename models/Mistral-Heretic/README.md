---
license: other
license_name: apache-2.0
license_link: https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3/blob/main/LICENSE
base_model: mistralai/Mistral-7B-Instruct-v0.3
tags:
  - mistral
  - chat
  - heretic
  - uncensored
  - decensored
  - abliterated
  - conversational
  - text-generation-inference
language:
  - en
pipeline_tag: text-generation
---

# Mistral-7B-Instruct-v0.3-heretic

<div align="center">
  <img src="https://mirror.sdad.pro/racer_is_op_banaer_model_card_and_dataset_card.png" alt="RACER IS OP" width="100%">
</div>

<br>

A decensored variant of [mistralai/Mistral-7B-Instruct-v0.3](https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3), produced with [Heretic](https://github.com/p-e-w/heretic) v1.4.0 (directional ablation / "abliteration"). Refusal behavior is suppressed via targeted weight edits to the attention output and MLP down-projections rather than fine-tuning, so the base model's knowledge and instruction-following are left largely intact.

**Who this is for:** developers who want Mistral's 7B instruct architecture without the refusal guardrails - for local agents, roleplay, function-calling, and research on alignment/refusal mechanics. At 7B it's a strong general-purpose uncensored model for consumer hardware.

## Why abliteration instead of fine-tuning

Fine-tuning a "helpful" persona on top of RLHF'd refusals fights the base model's training and tends to degrade coherence. Abliteration instead finds and edits the specific weight directions responsible for refusal, leaving the rest of the network (and its capabilities) untouched. See the [Heretic repo](https://github.com/p-e-w/heretic) and the [original abliteration writeup](https://huggingface.co/blog/mlabonne/abliteration) for the mechanism.

## Abliteration parameters

| Parameter | Value |
|---|---|
| `direction_index` | 17.87 |
| `attn.o_proj.max_weight` | 1.35 |
| `attn.o_proj.max_weight_position` | 22.34 |
| `attn.o_proj.min_weight` | 1.29 |
| `attn.o_proj.min_weight_distance` | 16.13 |
| `mlp.down_proj.max_weight` | 1.35 |
| `mlp.down_proj.max_weight_position` | 19.64 |
| `mlp.down_proj.min_weight` | 0.64 |
| `mlp.down_proj.min_weight_distance` | 7.34 |

## Performance

| Metric | This model | Mistral-7B-Instruct-v0.3 (base) |
|---|---|---|
| Refusals (out of 100 adversarial prompts) | 3/100 | 86/100 |
| KL divergence from base | 0.0687 | 0 *(by definition)* |

KL divergence of 0.07 on the output distribution is low for a 7B model - the edit is narrow and targeted. Refusals dropped from 86 to 3 out of 100 adversarial prompts while retaining nearly all original capabilities.

> Made with ❤️ by **RACER IS OP** - follow for more uncensored models

## Files

### Safetensors (BF16)

The full-precision weights are in `model-0000N-of-0000N.safetensors` (see the repo file listing for the exact shard count and sizes).

### GGUF quantizations

GGUF quantizations are published for this model (Q4_K_M, Q5_K_M, Q6_K, Q8_0). Exact sizes are in the repo file listing. Pull a specific quant with `llama.cpp` / `ollama` (see Quickstart).

| File | Format | Size |
|---|---|---|
| `Mistral-7B-Instruct-v0.3-heretic-Q4_K_M.gguf` | GGUF Q4_K_M | (see repo files for exact size) |
| `Mistral-7B-Instruct-v0.3-heretic-Q5_K_M.gguf` | GGUF Q5_K_M | (see repo files for exact size) |
| `Mistral-7B-Instruct-v0.3-heretic-Q6_K.gguf` | GGUF Q6_K | (see repo files for exact size) |
| `Mistral-7B-Instruct-v0.3-heretic-Q8_0.gguf` | GGUF Q8_0 | (see repo files for exact size) |

## Quickstart

```bash
# llama.cpp - defaults to the Q4_K_M quant if multiple are present
llama serve -hf saidutta69/Mistral-7B-Instruct-v0.3-heretic:Q4_K_M
```

```python
# transformers
from transformers import AutoModelForCausalLM, AutoTokenizer

model_name = "saidutta69/Mistral-7B-Instruct-v0.3-heretic"
model = AutoModelForCausalLM.from_pretrained(model_name, torch_dtype="auto", device_map="auto")
tokenizer = AutoTokenizer.from_pretrained(model_name)

messages = [{"role": "user", "content": "Who are you?"}]
inputs = tokenizer.apply_chat_template(messages, add_generation_prompt=True, tokenize=True,
                                        return_dict=True, return_tensors="pt").to(model.device)
out = model.generate(**inputs, max_new_tokens=200)
print(tokenizer.decode(out[0][inputs["input_ids"].shape[-1]:], skip_special_tokens=True))
```

Also runnable via Ollama, LM Studio, Jan, vLLM, SGLang - see the "Use this model" widget above for copy-paste commands.

## Responsible use

Refusal suppression is deliberate and works as intended: this model will comply with requests the base model would refuse, including some it shouldn't. There is no safety filtering layered on top. You are responsible for how you deploy it - don't put this behind an unmoderated public-facing endpoint serving third parties. It inherits mistralai/Mistral-7B-Instruct-v0.3's factual limitations and biases; abliteration removes refusal directions, it doesn't add capability or judgment.

## License

Inherits the [Apache 2.0](https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3/blob/main/LICENSE) license from the base model.

## Related

- [Qwen2.5-7B-Instruct-heretic](https://huggingface.co/saidutta69/Qwen2.5-7B-Instruct-heretic)
- [Mistral-7B-Instruct-v0.2-heretic](https://huggingface.co/saidutta69/Mistral-7B-Instruct-v0.2-heretic)

---

# Base model: Mistral-7B-Instruct-v0.3

<details>
<summary>Original Mistral-7B-Instruct-v0.3 model card (click to expand)</summary>

See the base model card at [mistralai/Mistral-7B-Instruct-v0.3](https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3) for the original architecture, training details, requirements, and citation.
</details>
