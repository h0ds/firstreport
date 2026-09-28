# Test models

`tiny.onnx` has the same input and output signature as the MiniLM-L6
sentence transformer (`input_ids`, `attention_mask`, `token_type_ids` →
`last_hidden_state`) but is a 64-token, 16-dim embedding lookup so the
embedder can be tested offline without the real weights. `tiny-table.npy`
is the lookup table it was built from. Regenerate with the script in the
commit that added them (`make_tiny.py`, needs the `onnx` and `onnxruntime` wheels) if the signature changes.
