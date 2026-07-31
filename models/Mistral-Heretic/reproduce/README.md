# Reproduction guide

This directory contains the necessary information and assets to reproduce the results obtained during this Heretic run.

## Models

- **Base model:** [mistralai/Mistral-7B-Instruct-v0.3](https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3) (Commit: [`c170c70`](https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3/commit/c170c708c41dac9275d15a8fff4eca08d52bab71))

## Datasets

- **Good prompts:** [mlabonne/harmless_alpaca](https://huggingface.co/datasets/mlabonne/harmless_alpaca) (Commit: [`02c6a92`](https://huggingface.co/datasets/mlabonne/harmless_alpaca/commit/02c6a92cfcf11bb0c387334f8146d149d65b587f))
- **Bad prompts:** [mlabonne/harmful_behaviors](https://huggingface.co/datasets/mlabonne/harmful_behaviors) (Commit: [`01cead0`](https://huggingface.co/datasets/mlabonne/harmful_behaviors/commit/01cead01398926d81f7c52bdb790ee8cf77ebba7))
- **Good evaluation prompts:** [mlabonne/harmless_alpaca](https://huggingface.co/datasets/mlabonne/harmless_alpaca) (Commit: [`02c6a92`](https://huggingface.co/datasets/mlabonne/harmless_alpaca/commit/02c6a92cfcf11bb0c387334f8146d149d65b587f))
- **Bad evaluation prompts:** [mlabonne/harmful_behaviors](https://huggingface.co/datasets/mlabonne/harmful_behaviors) (Commit: [`01cead0`](https://huggingface.co/datasets/mlabonne/harmful_behaviors/commit/01cead01398926d81f7c52bdb790ee8cf77ebba7))

## Selected trial

- **Trial number:** 106
- **KL divergence:** 0.068717
- **Refusals:** 3/100

## System

- **Python:** 3.12.11 (CPython, GCC 11.2.0) [Conda]
- **Operating system:** Linux-6.8.0-1058-aws-x86_64-with-glibc2.39 (x86_64)
- **CPU:** Intel(R) Xeon(R) Platinum 8559C

### Accelerators

- **CUDA:** Detected 1 device(s) (94.97 GB total VRAM)
  - **CUDA Version:** 12.8
  - **Driver Version:** 580.126.20
- **Devices:**
  - **CUDA 0:** NVIDIA RTX PRO 6000 Blackwell Server Edition (94.97 GB)

## Environment

- **Heretic:** v1.4.0 (Origin: PyPI)
- **PyTorch:** 2.8.0+cu128
- **Other dependencies:** See [`requirements.txt`](requirements.txt).

## Contents of this directory

- [`requirements.txt`](requirements.txt): The exact versions of all Python packages.
- [`config.toml`](config.toml): The exact configuration used, including the RNG seed.
- [`mistralai--Mistral-7B-Instruct-v0--3.jsonl`](mistralai--Mistral-7B-Instruct-v0--3.jsonl): The Optuna study journal containing the history of all trials.
- [`SHA256SUMS`](SHA256SUMS): Cryptographic hashes for all weight files.
- [`reproduce.json`](reproduce.json): A machine-readable file containing all reproducibility information.

## How to reproduce

> [!TIP]
> You can automate this process, including all verification steps, by downloading the `reproduce.json` file and running
> `heretic --reproduce reproduce.json`.

1. Ensure your system matches the specifications in the **System** section above. Exact reproducibility is only guaranteed if all aspects of your system are identical to the one the model was originally generated on.
1. Install the exact version of Heretic indicated in the **Environment** section above, from its original source.
1. Install the packages listed in `requirements.txt`: `pip install -r requirements.txt`
1. Install the correct version of PyTorch: `pip install torch==2.8.0+cu128 --index-url https://download.pytorch.org/whl/cu128`
1. Place the provided `config.toml` in your working directory.
1. Run Heretic without any additional arguments: `heretic`
1. Wait for the run to finish, then select trial **106** and export the model.
1. Verify that the weight files have been exactly reproduced by comparing their SHA-256 hashes against those in `SHA256SUMS`:
   `sha256sum -c SHA256SUMS` (or look at the hashes online if you uploaded to Hugging Face)

> [!TIP]
> To use the included Optuna study journal `mistralai--Mistral-7B-Instruct-v0--3.jsonl`, place it in the checkpoints directory (usually `checkpoints/`) before running Heretic.
>
> This allows you to export other models from the Pareto front, or to run additional trials without having to re-run the stored trials.
