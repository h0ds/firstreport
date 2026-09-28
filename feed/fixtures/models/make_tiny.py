"""Regenerate the tiny test model fixtures in this directory.

    pip install onnx onnxruntime numpy
    python make_tiny.py

Writes tiny-vocab.txt (64 WordPiece tokens), tiny-table.npy (64x16
float32 lookup table, fixed seed) and tiny.onnx, a model with the
MiniLM-L6 sentence-transformer signature:

    input_ids, attention_mask, token_type_ids  (int64 [batch, seq])
      -> last_hidden_state                     (float32 [batch, seq, 16])

The hidden state is table[input_ids] scaled by the attention mask, with
token_type_ids folded in as a zero term so every input is consumed.

The vocab is shaped by the tokenizer tests in src/embed/tokenizer.rs:
"holds"/"rates" must split into stem + "##s", "unaffordable" into
un ##aff ##ord ##able, and nothing may match "zzzzzz".
"""

from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

HERE = Path(__file__).resolve().parent
DIM = 16

SPECIAL = ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "[MASK]"]
WORDS = [
    "the", "a", "of", "to", "in", "and", "on", "for", "is", "at",
    "central", "bank", "hold", "rate", "interest", "inflation", "market",
    "flood", "storm", "rain", "river", "city", "police", "court", "vote",
    "election", "minister", "president", "war", "peace", "talks", "deal",
    "hello", "world", "news", "kagami", "un",
    ",", ".", "!", "?", "'", "-", ":",
]
PIECES = ["##s", "##ed", "##ing", "##aff", "##ord", "##able", "##er", "##ly"]

vocab = SPECIAL + WORDS + PIECES
filler = 64 - len(vocab)
assert filler >= 0, f"vocab over 64 by {-filler}"
vocab += [f"[unused{i}]" for i in range(filler)]
assert len(vocab) == 64 and len(set(vocab)) == 64

rng = np.random.default_rng(20260929)
table = rng.standard_normal((64, DIM)).astype(np.float32)
table[0] = 0.0  # [PAD]

(HERE / "tiny-vocab.txt").write_text("\n".join(vocab) + "\n", encoding="utf-8", newline="\n")
np.save(HERE / "tiny-table.npy", table)

dyn = ["batch", "seq"]
inputs = [
    helper.make_tensor_value_info(n, TensorProto.INT64, dyn)
    for n in ("input_ids", "attention_mask", "token_type_ids")
]
output = helper.make_tensor_value_info("last_hidden_state", TensorProto.FLOAT, dyn + [DIM])
nodes = [
    helper.make_node("Gather", ["table", "input_ids"], ["emb"], axis=0),
    helper.make_node("Cast", ["attention_mask"], ["mask_f"], to=TensorProto.FLOAT),
    helper.make_node("Cast", ["token_type_ids"], ["tt_f"], to=TensorProto.FLOAT),
    helper.make_node("Mul", ["tt_f", "zero"], ["tt_zero"]),
    helper.make_node("Add", ["mask_f", "tt_zero"], ["scale"]),
    helper.make_node("Unsqueeze", ["scale", "axis_last"], ["scale3"]),
    helper.make_node("Mul", ["emb", "scale3"], ["last_hidden_state"]),
]
inits = [
    numpy_helper.from_array(table, "table"),
    numpy_helper.from_array(np.array(0.0, dtype=np.float32), "zero"),
    numpy_helper.from_array(np.array([-1], dtype=np.int64), "axis_last"),
]
graph = helper.make_graph(nodes, "tiny", inputs, [output], inits)
model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)], producer_name="kagami")
model.ir_version = 8
onnx.checker.check_model(model)
onnx.save(model, HERE / "tiny.onnx")

# Sanity check against the lookup table.
import onnxruntime as ort

sess = ort.InferenceSession(str(HERE / "tiny.onnx"), providers=["CPUExecutionProvider"])
ids = np.array([[2, 15, 3, 0]], dtype=np.int64)
mask = np.array([[1, 1, 1, 0]], dtype=np.int64)
(out,) = sess.run(None, {"input_ids": ids, "attention_mask": mask, "token_type_ids": np.zeros_like(ids)})
assert out.shape == (1, 4, DIM)
assert np.allclose(out[0, :3], table[ids[0, :3]]) and not out[0, 3].any()
print("wrote tiny-vocab.txt, tiny-table.npy, tiny.onnx")
