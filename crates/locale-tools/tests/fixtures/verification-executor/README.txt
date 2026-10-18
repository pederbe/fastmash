Independent completed verification outputs

mixed.json and over120.json are unchanged completed JSON reports captured from
scripts/verify_collation_locales.py. success.stdout is the complete identical
stdout of both runs. Expected bytes were not produced by the Rust executor.

Producer SHA256
  083c685a2a15cd5db564049267eb521434eb50396adadee9c13d5899ddfdb026
Interpreter
  CPython 3.12.14, built-in unicodedata, Unicode 15.0.0
  SHA256 fa67443527ed9647f760d807e2a38f26340757123e643c4639cf273ed15d5ea7
Identity observer source SHA256
  f5f8e3da0176a7cfbe4f2b6c0a700db46e2de4ebee267beb109ba442e58440c9
Capture date
  2026-10-18

Each run selected only en_US, variant 0, with one locale worker and all 31
case pairs. The observer returned each supplied word with a TAB and count 1.
This is functional report evidence, not native collator correctness or parity.
The literal configurations and all distinct alphabet/block input vectors are
in ../unicode15. Its README and source-input-vectors.tsv describe their origin.
The test reuses the maintained observer's unchanged default success behavior.

SHA256
  mixed.json: 106f784aef76c4d69228935713affec6d6a9fd50e712cc4c0ad7bf0fe2d09fc5
  over120.json: d11d9598283c94d0019765e518b7daf582ec9134ae199912b5635201cd0c1614
  success.stdout: 78270b653521f8b0fd5457b25b894913ce035137cc7130b9024b4953de92734a
