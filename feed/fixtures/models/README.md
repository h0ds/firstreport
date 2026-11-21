# Test models

`tiny.onnx` has the same input and output signature as the MiniLM-L6
sentence transformer (`input_ids`, `attention_mask`, `token_type_ids` →
`last_hidden_state`) but is a 64-token, 16-dim embedding lookup so the
embedder can be tested offline without the real weights. `tiny-table.npy`
is the lookup table it was built from. Regenerate with `python make_tiny.py`
(needs the `onnx`, `onnxruntime` and `numpy` wheels) if the signature or
vocab changes. The ONNX tests run only with
`FIRSTREPORT_ORT_LIB=<path to the runtime library> cargo test`.
