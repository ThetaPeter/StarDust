# Luna Light Enc/Post on the same WorldLay. BoundClass: persist_encodings,
# check_store, check_dict, smoke_luna. Spec: Archive luna_light.sd / luna_light_logic.md.
from __future__ import annotations

import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
if _HERE not in sys.path:
    sys.path.insert(0, _HERE)

from cns import World, set_u64
from layout_off import OFF_LUNA_SMOKE_CHK, OFF_MISMATCH_COUNT, WORD_POST_ENCODING

sys.setrecursionlimit(50000)


class Luna:
    def __init__(self, w: World):
        self.w = w
        self._enc_memo = {}
        self._post_memo = {}
        self._full_memo = {}

    def idc_arity(self, c: str) -> int:
        if c in ("⿲", "⿳"):
            return 3
        return 2

    def concat_leaves_str_for_level(self, char: str, level: int) -> str:
        ids_line = self.w.effective_ids(char)
        if not ids_line:
            return ""
        if not self.w.is_idc(ids_line[0]):
            return ""
        n = self.w.ids_leaf_count(char)
        if level == 2:
            perm = self.w.wi_rows(char)
            if perm:
                out = []
                for _seq, leaf_index in perm:
                    out.append(self.w.ids_leaf_at(char, leaf_index - 1))
                return "".join(out)
        return "".join(self.w.ids_leaf_at(char, i) for i in range(n))

    def concat_leaves_str(self, char: str) -> str:
        return self.concat_leaves_str_for_level(char, self.word_sequence_level(char))

    def concat_leaf_count(self, char: str) -> int:
        return len(self.concat_leaves_str(char))

    def concat_leaf_at(self, char: str, idx: int) -> str:
        leaves = self.concat_leaves_str(char)
        return leaves[idx] if 0 <= idx < len(leaves) else ""

    def ids_line_impure(self, ids_line: str) -> bool:
        return self.w.ids_line_impure(ids_line)

    def wc_comp_keys_str(self, char: str) -> str:
        return self.w.wc_comp_keys_str(char)

    def default_sequence_level(self, char: str) -> int:
        return self.w.default_sequence_level(char)

    def word_sequence_level(self, char: str) -> int:
        return self.w.word_sequence_level(char)

    def word_split_index(self, char: str) -> int:
        return self.w.split_index(char)

    def ids_formula_comp_keys_str(self, char: str) -> str:
        return self.w.components_of_ids_formula(char)

    def ids_perm_comp_keys_str(self, char: str) -> str:
        return self.w.components_of_ids_perm(char)

    def ids_comp_keys_str(self, char: str) -> str:
        return self.w.components_of_general(char)

    def comp_keys_str_for_level(self, char: str, level: int) -> str:
        return self.w.comp_keys_str_for_level(char, level)

    def comp_key_count(self, char: str) -> int:
        if self.w.is_entity_leaf(char):
            return 1 if self.w.entity_component_of(char) else 0
        lvl = self.word_sequence_level(char)
        return len(self.comp_keys_str_for_level(char, lvl))

    def comp_key_at(self, char: str, idx: int) -> str:
        if self.w.is_entity_leaf(char):
            return self.w.entity_component_of(char)
        lvl = self.word_sequence_level(char)
        sc = self.comp_keys_str_for_level(char, lvl)
        return sc[idx] if 0 <= idx < len(sc) else ""

    def first_comp_key(self, char: str) -> str:
        return self.comp_key_at(char, 0)

    def first_head_pick_key(self, char: str) -> str:
        lvl = self.word_sequence_level(char)
        sc = self.comp_keys_str_for_level(char, lvl)
        if sc:
            return sc[0]
        return self.first_comp_key(char)

    def normalize_enc(self, enc: str) -> str:
        return enc or ""

    def component_encoding(self, key: str) -> str:
        return self.normalize_enc(self.w.component_encoding(key))

    def full_encoding_atomic(self, char: str) -> str:
        n = self.comp_key_count(char)
        if n == 0:
            return self.component_encoding(char)
        return "".join(self.component_encoding(self.comp_key_at(char, i)) for i in range(n))

    def is_multi_part_ids(self, char: str) -> bool:
        ids_line = self.w.effective_ids(char)
        return bool(ids_line) and self.w.is_idc(ids_line[0])

    def wc_row_count(self, char: str) -> int:
        return len(self.w.wc_rows(char))

    def full_encoding(self, char: str) -> str:
        hit = self._full_memo.get(char)
        if hit is not None:
            return hit
        lvl = self.word_sequence_level(char)
        if lvl == 3:
            out = self.full_encoding_atomic(char)
        elif self.is_multi_part_ids(char):
            n = self.concat_leaf_count(char)
            out = "".join(
                self.encoding_of(self.concat_leaf_at(char, i)) for i in range(n)
            )
        else:
            out = self.full_encoding_atomic(char)
        self._full_memo[char] = out
        return out

    def enc_slot(self, s: str, idx: int) -> str:
        if idx < 0 or idx >= len(s):
            return ""
        return s[idx]

    def take_encoding(self, full: str) -> str:
        ln = len(full)
        if ln == 0:
            return ""
        if ln < 4:
            return full
        return (
            self.enc_slot(full, 0)
            + self.enc_slot(full, 1)
            + self.enc_slot(full, 2)
            + self.enc_slot(full, ln - 1)
        )

    def encoding_override_of(self, char: str) -> str:
        enc, _post = self.w.override_of(char)
        return self.normalize_enc(enc)

    def encoding_of(self, char: str) -> str:
        hit = self._enc_memo.get(char)
        if hit is not None:
            return hit
        ovr = self.encoding_override_of(char)
        if ovr:
            self._enc_memo[char] = ovr
            return ovr
        out = self.take_encoding(self.full_encoding(char))
        self._enc_memo[char] = out
        return out

    def subtree_end(self, ids: str, idx: int) -> int:
        if idx >= len(ids):
            return idx
        c = ids[idx]
        if self.w.is_idc(c):
            return self._subtree_end_arity(ids, idx + 1, 0, self.idc_arity(c))
        if self.w.is_entity_start(c):
            return self.w.entity_end(ids, idx)
        return idx + 1

    def _subtree_end_arity(self, ids: str, pos: int, got: int, need: int) -> int:
        while got < need:
            pos = self.subtree_end(ids, pos)
            got += 1
        return pos

    def subtree_at(self, ids: str, idx: int) -> str:
        return ids[idx : self.subtree_end(ids, idx)]

    def same_idc_group_op(self, a: str, b: str) -> bool:
        if a in ("⿰", "⿲"):
            return b in ("⿰", "⿲")
        if a in ("⿱", "⿳"):
            return b in ("⿱", "⿳")
        if a in ("⿴", "⿵", "⿶", "⿷", "⿸", "⿹", "⿺"):
            return a == b
        return False

    def has_encoding_override(self, char: str) -> bool:
        return bool(self.encoding_override_of(char))

    def can_ids_split(self, ids: str) -> bool:
        if not ids:
            return False
        c0 = ids[0]
        if c0 == "⿻" or not self.w.is_idc(c0):
            return False
        return c0 in "⿰⿲⿱⿳⿴⿵⿶⿷⿸⿹⿺"

    def ids_expand_ref(self, s: str) -> str:
        if not s:
            return ""
        if self.w.is_idc(s[0]):
            return s
        return self.w.effective_ids(s)

    def chain_head_subtree(self, ids: str) -> str:
        if not ids:
            return ""
        c0 = ids[0]
        if c0 == "⿻" or not self.w.is_idc(c0):
            return ids
        child = self.subtree_at(ids, 1)
        c1 = child[0] if child else ""
        if self.w.is_idc(c1):
            if self.same_idc_group_op(c1, c0):
                return self.chain_head_subtree(child)
            return child
        if self.has_encoding_override(child):
            return child
        ref_ids = self.w.effective_ids(child)
        if not ref_ids:
            return child
        r0 = ref_ids[0]
        if self.w.is_idc(r0):
            if self.same_idc_group_op(r0, c0):
                return self.chain_head_subtree(ref_ids)
            return child
        return child

    def split_units_idc_children(self, ids: str, pos: int, arity: int) -> str:
        parts = []
        got = 0
        while got < arity:
            child = self.subtree_at(ids, pos)
            parts.append(child)
            pos = self.subtree_end(ids, pos)
            got += 1
        return "§".join(parts)

    def split_remainder_in_child(self, child_ids: str, head_subtree: str) -> str:
        expanded = self.ids_expand_ref(child_ids)
        if not expanded or not self.w.is_idc(expanded[0]):
            return ""
        ar = self.idc_arity(expanded[0])
        first_child = self.subtree_at(expanded, 1)
        first_end = self.subtree_end(expanded, 1)
        if first_child == head_subtree:
            return self.split_units_idc_children(expanded, first_end, ar - 1) if ar > 1 else ""
        inner = self.split_remainder_in_child(first_child, head_subtree)
        tail = self.split_units_idc_children(expanded, first_end, ar - 1) if ar > 1 else ""
        if not inner:
            return tail
        if not tail:
            return inner
        return inner + "§" + tail

    def split_remainder_units(self, ids: str, head_subtree: str) -> str:
        if not ids or not self.w.is_idc(ids[0]):
            return ""
        ar = self.idc_arity(ids[0])
        first_child = self.subtree_at(ids, 1)
        first_end = self.subtree_end(ids, 1)
        if first_child == head_subtree:
            return self.split_units_idc_children(ids, first_end, ar - 1) if ar > 1 else ""
        inner = self.split_remainder_in_child(first_child, head_subtree)
        tail = self.split_units_idc_children(ids, first_end, ar - 1) if ar > 1 else ""
        if not inner:
            return tail
        if not tail:
            return inner
        return inner + "§" + tail

    def split_units_from_ids(self, ids: str) -> str:
        if not self.can_ids_split(ids):
            return ids
        head = self.chain_head_subtree(ids)
        if head == ids:
            return ids
        rem = self.split_remainder_units(ids, head)
        if not rem:
            return head
        return head + "§" + rem

    def immediate_units_from_ids(self, ids: str) -> str:
        if not self.can_ids_split(ids):
            return ids
        c0 = ids[0]
        ar = self.idc_arity(c0)
        return self.split_units_idc_children(ids, 1, ar)

    def unit_count(self, units_str: str) -> int:
        if units_str == "":
            return 0
        return len(units_str.split("§"))

    def unit_at(self, units_str: str, idx: int) -> str:
        parts = units_str.split("§")
        return parts[idx] if 0 <= idx < len(parts) else ""

    def split_units_for_char(self, char: str) -> str:
        ids_line = self.w.effective_ids(char)
        chained = self.split_units_from_ids(ids_line)
        uc = self.unit_count(chained)
        if uc < 2:
            return chained
        fk = self.first_head_pick_key(char)
        hi = self.head_pick_idx(chained, char, fk, 0)
        if hi < uc:
            return chained
        imm = self.immediate_units_from_ids(ids_line)
        iuc = self.unit_count(imm)
        if iuc >= 2 and imm != chained:
            return imm
        return chained

    def leaf_under_unit_loop(self, leaves_str: str, leaf: str) -> bool:
        n = self.w.leaf_token_count(leaves_str)
        for idx in range(n):
            if self.w.leaf_token_at(leaves_str, idx) == leaf:
                return True
        return False

    def key_under_leaves(self, leaves_str: str, key: str) -> bool:
        n = self.w.leaf_token_count(leaves_str)
        for idx in range(n):
            ww = self.w.leaf_token_at(leaves_str, idx)
            if ww == key:
                return True
            if key in self.w.components_of(ww):
                return True
        return False

    def key_under_unit(self, _parent: str, unit_ids: str, key: str) -> bool:
        leaves_str = self.w.leaf_tokens_in_ids_str(unit_ids)
        return self.key_under_leaves(leaves_str, key)

    def head_pick_idx(self, units_str: str, char: str, fk: str, idx: int) -> int:
        uc = self.unit_count(units_str)
        while idx < uc:
            u = self.unit_at(units_str, idx)
            if self.key_under_unit(char, u, fk):
                return idx
            idx += 1
        return uc

    def head_unit_index(self, char: str) -> int:
        units_str = self.split_units_for_char(char)
        uc = self.unit_count(units_str)
        if uc < 2:
            return uc
        fk = self.first_head_pick_key(char)
        return self.head_pick_idx(units_str, char, fk, 0)

    def unit_segment_encoding(self, unit_ids: str) -> str:
        leaves_str = self.w.leaf_tokens_in_ids_str(unit_ids)
        n = self.w.leaf_token_count(leaves_str)
        if n == 0:
            return ""
        if n == 1:
            return self.encoding_of(self.w.leaf_token_at(leaves_str, 0))
        return "".join(
            self.encoding_of(self.w.leaf_token_at(leaves_str, i)) for i in range(n)
        )

    def unit_segment_at(self, _char: str, units_str: str, unit_idx: int) -> str:
        return self.unit_segment_encoding(self.unit_at(units_str, unit_idx))

    def tail_segment(self, char: str, units_str: str, head_idx: int) -> str:
        uc = self.unit_count(units_str)
        out = []
        for idx in range(uc):
            if idx == head_idx:
                continue
            out.append(self.unit_segment_at(char, units_str, idx))
        return "".join(out)

    def level3_has_split(self, char: str) -> bool:
        si = self.word_split_index(char)
        if si <= 0:
            return False
        return si < self.wc_row_count(char)

    def comp_encoding_range(self, char: str, start: int, n: int) -> str:
        return "".join(
            self.component_encoding(self.comp_key_at(char, start + i)) for i in range(n)
        )

    def level3_head_encoding(self, char: str) -> str:
        si = self.word_split_index(char)
        return self.comp_encoding_range(char, 0, si)

    def level3_tail_encoding(self, char: str) -> str:
        si = self.word_split_index(char)
        n_keys = self.wc_row_count(char)
        return self.comp_encoding_range(char, si, n_keys - si)

    def leaf_encoding_range(self, char: str, start: int, n: int) -> str:
        return "".join(
            self.encoding_of(self.concat_leaf_at(char, start + i)) for i in range(n)
        )

    def level2_has_explicit_split(self, char: str) -> bool:
        si = self.word_split_index(char)
        if si <= 0:
            return False
        return si < self.concat_leaf_count(char)

    def level2_split_head_encoding(self, char: str) -> str:
        return self.leaf_encoding_range(char, 0, self.word_split_index(char))

    def level2_split_tail_encoding(self, char: str) -> str:
        si = self.word_split_index(char)
        n = self.concat_leaf_count(char)
        return self.leaf_encoding_range(char, si, n - si)

    def l2_remove_leaf_once(self, leaves_str: str, leaf: str) -> str:
        n = self.w.leaf_token_count(leaves_str)
        parts = [self.w.leaf_token_at(leaves_str, i) for i in range(n)]
        try:
            parts.remove(leaf)
        except ValueError:
            return leaves_str
        if "§" in leaves_str or any(len(p) != 1 for p in parts):
            return "§".join(parts)
        return "".join(parts)

    def l2_membership_head_encoding(self, char: str) -> str:
        units_str = self.split_units_for_char(char)
        uc = self.unit_count(units_str)
        head_idx = self.head_unit_index(char)
        if head_idx >= uc:
            return ""
        u = self.unit_at(units_str, head_idx)
        head_leaves = self.w.leaf_tokens_in_ids_str(u)
        n = self.concat_leaf_count(char)
        out = []
        bag = head_leaves
        for idx in range(n):
            leaf = self.concat_leaf_at(char, idx)
            hn = self.w.leaf_token_count(bag)
            if self.leaf_under_unit_loop(bag, leaf):
                out.append(self.encoding_of(leaf))
                bag = self.l2_remove_leaf_once(bag, leaf)
        return "".join(out)

    def l2_membership_tail_encoding(self, char: str) -> str:
        units_str = self.split_units_for_char(char)
        uc = self.unit_count(units_str)
        head_idx = self.head_unit_index(char)
        if head_idx >= uc:
            return ""
        u = self.unit_at(units_str, head_idx)
        head_leaves = self.w.leaf_tokens_in_ids_str(u)
        n = self.concat_leaf_count(char)
        out = []
        bag = head_leaves
        for idx in range(n):
            leaf = self.concat_leaf_at(char, idx)
            hn = self.w.leaf_token_count(bag)
            if self.leaf_under_unit_loop(bag, leaf):
                bag = self.l2_remove_leaf_once(bag, leaf)
            else:
                out.append(self.encoding_of(leaf))
        return "".join(out)

    def full_encoding_head(self, char: str) -> str:
        lvl = self.word_sequence_level(char)
        if lvl == 3:
            if self.level3_has_split(char):
                return self.level3_head_encoding(char)
            return self.full_encoding(char)
        if not self.is_multi_part_ids(char):
            return self.full_encoding(char)
        if lvl == 2:
            if self.level2_has_explicit_split(char):
                return self.level2_split_head_encoding(char)
            return self.l2_membership_head_encoding(char)
        units_str = self.split_units_for_char(char)
        uc = self.unit_count(units_str)
        head_idx = self.head_unit_index(char)
        if head_idx >= uc:
            return ""
        return self.unit_segment_at(char, units_str, head_idx)

    def full_encoding_tail(self, char: str) -> str:
        lvl = self.word_sequence_level(char)
        if lvl == 3:
            if self.level3_has_split(char):
                return self.level3_tail_encoding(char)
            return ""
        if not self.is_multi_part_ids(char):
            return ""
        if lvl == 2:
            if self.level2_has_explicit_split(char):
                return self.level2_split_tail_encoding(char)
            return self.l2_membership_tail_encoding(char)
        units_str = self.split_units_for_char(char)
        uc = self.unit_count(units_str)
        head_idx = self.head_unit_index(char)
        if head_idx >= uc:
            return ""
        return self.tail_segment(char, units_str, head_idx)

    def has_split(self, char: str) -> bool:
        lvl = self.word_sequence_level(char)
        if lvl == 3:
            return self.level3_has_split(char)
        if lvl == 2:
            if self.level2_has_explicit_split(char):
                return True
            if not self.is_multi_part_ids(char):
                return False
            return self.unit_count(self.split_units_for_char(char)) >= 2
        if not self.is_multi_part_ids(char):
            return False
        return self.unit_count(self.split_units_for_char(char)) >= 2

    def take_post_head(self, head_seg: str) -> str:
        if len(head_seg) >= 2:
            return self.enc_slot(head_seg, 0) + self.enc_slot(head_seg, len(head_seg) - 1)
        return head_seg

    def take_post_tail(self, tail_seg: str) -> str:
        if len(tail_seg) >= 3:
            return (
                self.enc_slot(tail_seg, 0)
                + self.enc_slot(tail_seg, 1)
                + self.enc_slot(tail_seg, len(tail_seg) - 1)
            )
        return tail_seg

    def take_post_split(self, head_seg: str, tail_seg: str) -> str:
        return self.take_post_head(head_seg) + self.take_post_tail(tail_seg)

    def post_override_of(self, char: str) -> str:
        _enc, post = self.w.override_of(char)
        return self.normalize_enc(post)

    def post_encoding_of(self, char: str) -> str:
        hit = self._post_memo.get(char)
        if hit is not None:
            return hit
        ovr = self.post_override_of(char)
        if ovr:
            self._post_memo[char] = ovr
            return ovr
        if self.has_split(char):
            head_seg = self.full_encoding_head(char)
            tail_seg = self.full_encoding_tail(char)
            out = self.take_post_split(head_seg, tail_seg)
        else:
            out = self.encoding_of(char)
        self._post_memo[char] = out
        return out

    def persist_encodings(self) -> None:
        for ch in list(self.w.iter_word_chars()):
            enc = self.encoding_of(ch)
            post = self.post_encoding_of(ch)
            self.w.set_word_enc_post(ch, enc, post)

    def check_store(self) -> int:
        mismatches = 0
        for ch in self.w.iter_word_chars():
            pers_enc = self.normalize_enc(self.w.word_encoding(ch))
            i = self.w._word_i[ch]
            pers_post = self.normalize_enc(
                self.w.get_slice(self.w.word_base(i) + WORD_POST_ENCODING)
            )
            if not pers_enc and not pers_post:
                continue
            der_enc = self.encoding_of(ch)
            der_post = self.post_encoding_of(ch)
            if der_enc != pers_enc or der_post != pers_post:
                mismatches += 1
                if mismatches <= 20:
                    print(
                        f"MISMATCH {ascii(ch)}: derived Enc={der_enc} Post={der_post} "
                        f"persisted Enc={pers_enc} Post={pers_post}"
                    )
        set_u64(self.w.buf, OFF_MISMATCH_COUNT, mismatches)
        return mismatches

    def dict_yaml_path(self) -> str:
        return os.path.join(_HERE, "luna_light.dict.yaml")

    def split_rime_dict(self, text: str) -> tuple[str, list[str]]:
        lines = text.splitlines(keepends=True)
        body_start = None
        for i, line in enumerate(lines):
            if line.strip() == "...":
                body_start = i + 1
                break
        if body_start is None:
            raise ValueError("luna_light.dict.yaml: missing YAML ... marker")
        return "".join(lines[:body_start]), lines[body_start:]

    def parse_rime_body_line(self, line: str):
        raw = line.rstrip("\r\n")
        if not raw or raw.startswith("#"):
            return None
        parts = raw.split("\t")
        if len(parts) < 2:
            return None
        return parts[0], parts[1], parts[2:]

    def rebuild_rime_body_line(self, orig: str, ch: str, post: str, extra: list[str]) -> str:
        nl = "\r\n" if orig.endswith("\r\n") else ("\n" if orig.endswith("\n") else "")
        cells = [ch, post]
        cells.extend(extra)
        return "\t".join(cells) + nl

    def check_dict(self) -> int:
        path = self.dict_yaml_path()
        with open(path, "r", encoding="utf-8") as f:
            text = f.read()
        header, body_lines = self.split_rime_dict(text)
        mismatches = 0
        gen_body = []
        for line in body_lines:
            parsed = self.parse_rime_body_line(line)
            if parsed is None:
                gen_body.append(line)
                continue
            ch, golden_post, extra = parsed
            if not self.w.word_exist(ch):
                gen_body.append(line)
                continue
            derived = self.post_encoding_of(ch)
            gen_body.append(self.rebuild_rime_body_line(line, ch, derived, extra))
            if derived != golden_post:
                mismatches += 1
                if mismatches <= 20:
                    print(
                        f"DICT_MISMATCH {ascii(ch)}: derived Post={derived} golden Post={golden_post}",
                        file=sys.stderr,
                        flush=True,
                    )
        set_u64(self.w.buf, OFF_MISMATCH_COUNT, mismatches)
        if os.environ.get("STARDUST_DUMP_DICT") == "1":
            generated = header + "".join(gen_body)
            if generated != text:
                dump_path = os.path.join(_HERE, "luna_light.generated.dict.yaml")
                with open(dump_path, "w", encoding="utf-8", newline="") as f:
                    f.write(generated)
        return mismatches

    def expect_eq(self, label: str, got: str, want: str, chk: int) -> int:
        if got == want:
            return chk + len(want) + len(label)
        print(f"FAIL {ascii(label)}: got={got} want={want}")
        return chk

    def smoke_luna(self) -> int:
        chk = 0
        chk = self.expect_eq("轂 full", self.full_encoding("轂"), "vdjjrakob", chk)
        chk = self.expect_eq("轂 head", self.full_encoding_head("轂"), "vdjjra", chk)
        chk = self.expect_eq("轂 tail", self.full_encoding_tail("轂"), "kob", chk)
        chk = self.expect_eq("轂 enc", self.encoding_of("轂"), "vdjb", chk)
        chk = self.expect_eq("轂 post", self.post_encoding_of("轂"), "vakob", chk)
        chk = self.expect_eq("念 head", self.full_encoding_head("念"), "s", chk)
        chk = self.expect_eq("念 tail", self.full_encoding_tail("念"), "junn", chk)
        chk = self.expect_eq("念 enc", self.encoding_of("念"), "sjun", chk)
        chk = self.expect_eq("念 post", self.post_encoding_of("念"), "sjun", chk)
        chk = self.expect_eq("過 full", self.full_encoding("過"), "dtdfo", chk)
        chk = self.expect_eq("過 post", self.post_encoding_of("過"), "dfo", chk)
        chk = self.expect_eq("士 atomic", self.encoding_of("士"), "v", chk)
        chk = self.expect_eq("士 post", self.post_encoding_of("士"), "v", chk)
        chk = self.expect_eq("辨 full", self.full_encoding("辨"), "hwgnhwa", chk)
        chk = self.expect_eq("辨 head", self.full_encoding_head("辨"), "hwg", chk)
        chk = self.expect_eq("辨 tail", self.full_encoding_tail("辨"), "nhwa", chk)
        chk = self.expect_eq("辨 enc", self.encoding_of("辨"), "hwga", chk)
        chk = self.expect_eq("辨 post", self.post_encoding_of("辨"), "hgnha", chk)
        chk = self.expect_eq("趣 full", self.full_encoding("趣"), "vtstmyjb", chk)
        chk = self.expect_eq("趣 head", self.full_encoding_head("趣"), "vts", chk)
        chk = self.expect_eq("趣 tail", self.full_encoding_tail("趣"), "tmyjb", chk)
        chk = self.expect_eq("趣 enc", self.encoding_of("趣"), "vtsb", chk)
        chk = self.expect_eq("趣 post", self.post_encoding_of("趣"), "vstmb", chk)
        chk = self.expect_eq("take1", self.take_encoding("j"), "j", chk)
        chk = self.expect_eq("take2", self.take_encoding("ab"), "ab", chk)
        chk = self.expect_eq("take5", self.take_encoding("abcde"), "abce", chk)
        old = self.w.component_encoding("󶁤")
        if self.w._comp_i.get("󶁤") is not None:
            if old == "":
                old = "v"
            self.w.set_component_encoding("󶁤", "9")
            self._enc_memo.clear()
            self._post_memo.clear()
            self._full_memo.clear()
            exc_full = self.full_encoding("轂")
            self.w.set_component_encoding("󶁤", old)
            chk = self.expect_eq("轂 exc", exc_full, "9djjrakob", chk)
        set_u64(self.w.buf, OFF_LUNA_SMOKE_CHK, chk)
        return chk


def persist_encodings(view) -> None:
    Luna(World(view)).persist_encodings()


def check_store(view) -> None:
    Luna(World(view)).check_store()


def check_dict(view) -> None:
    Luna(World(view)).check_dict()


def smoke_luna(view) -> None:
    Luna(World(view)).smoke_luna()
