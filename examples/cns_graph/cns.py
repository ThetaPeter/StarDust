# CNS11643 graph on a WorldLay memoryview. BoundClass symbols: import_all,
# components_of, stroke_order, smoke. Query/result live in header slices.
from __future__ import annotations

import os
import struct
import sys
from collections import defaultdict

_HERE = os.path.dirname(os.path.abspath(__file__))
if _HERE not in sys.path:
    sys.path.insert(0, _HERE)

from layout_off import *  # noqa: F403, E402

sys.setrecursionlimit(50000)

DATA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")
IDC = set("⿰⿱⿲⿳⿴⿵⿶⿷⿸⿹⿺⿻")


def u64(buf, off: int) -> int:
    return struct.unpack_from("<Q", buf, off)[0]


def set_u64(buf, off: int, val: int) -> None:
    struct.pack_into("<Q", buf, off, val)


class World:
    def __init__(self, view):
        self.buf = view
        self._word_i = {}
        self._comp_i = {}
        self._wi = defaultdict(list)
        self._wc = defaultdict(list)
        self._ws = defaultdict(list)
        self._ent = {}
        self._ovr = {}
        self._indexed = False

    def n_words(self) -> int:
        return u64(self.buf, OFF_N_WORDS)

    def n_comps(self) -> int:
        return u64(self.buf, OFF_N_COMPS)

    def intern(self, s: str) -> tuple[int, int]:
        raw = s.encode("utf-8")
        used = u64(self.buf, OFF_BLOB_USED)
        if used + len(raw) > CAP_BLOB:
            raise MemoryError("blob full")
        start = OFF_BLOB + used
        self.buf[start : start + len(raw)] = raw
        set_u64(self.buf, OFF_BLOB_USED, used + len(raw))
        return start, len(raw)

    def set_slice(self, field_off: int, s: str) -> None:
        off, n = self.intern(s)
        set_u64(self.buf, field_off, off)
        set_u64(self.buf, field_off + 8, n)

    def get_slice(self, field_off: int) -> str:
        off = u64(self.buf, field_off)
        n = u64(self.buf, field_off + 8)
        if n == 0:
            return ""
        return bytes(self.buf[off : off + n]).decode("utf-8")

    def read_query(self) -> str:
        return self.get_slice(OFF_QUERY)

    def write_result(self, s: str) -> None:
        self.set_slice(OFF_RESULT, s)

    def word_base(self, i: int) -> int:
        return OFF_WORDS + i * STRIDE_WORDS

    def comp_base(self, i: int) -> int:
        return OFF_COMPONENTS + i * STRIDE_COMPONENTS

    def reindex(self) -> None:
        self._word_i.clear()
        self._comp_i.clear()
        self._wi = defaultdict(list)
        self._wc = defaultdict(list)
        self._ws = defaultdict(list)
        self._ent.clear()
        self._ovr.clear()
        for i in range(self.n_words()):
            ch = self.get_slice(self.word_base(i) + WORD_CHAR)
            self._word_i[ch] = i
        for i in range(self.n_comps()):
            ch = self.get_slice(self.comp_base(i) + COMP_CHAR)
            self._comp_i[ch] = i
        n_wi = u64(self.buf, OFF_N_WORD_IDS)
        for i in range(n_wi):
            b = OFF_WORD_IDS + i * STRIDE_WORD_IDS
            self._wi[self.get_slice(b + WIDS_FROM)].append(i)
        n_wc = u64(self.buf, OFF_N_WORD_COMP)
        for i in range(n_wc):
            b = OFF_WORD_COMP + i * STRIDE_WORD_COMP
            self._wc[self.get_slice(b + EDGE_FROM)].append(i)
        n_ws = u64(self.buf, OFF_N_WORD_STROKE)
        for i in range(n_ws):
            b = OFF_WORD_STROKE + i * STRIDE_WORD_STROKE
            self._ws[self.get_slice(b + WSTR_FROM)].append(i)
        n_ent = u64(self.buf, OFF_N_IDS_ENTITY)
        for i in range(n_ent):
            b = OFF_IDS_ENTITY + i * STRIDE_IDS_ENTITY
            self._ent[self.get_slice(b + ENT_ENTITY)] = self.get_slice(b + ENT_COMPONENT)
        n_ov = u64(self.buf, OFF_N_OVERRIDE)
        for i in range(n_ov):
            b = OFF_OVERRIDES + i * STRIDE_OVERRIDES
            self._ovr[self.get_slice(b + OVR_CHAR)] = (
                self.get_slice(b + OVR_ENCODING),
                self.get_slice(b + OVR_POST),
            )
        self._indexed = True

    def ensure_index(self) -> None:
        if not self._indexed:
            self.reindex()

    def word_ids_line(self, char: str) -> str:
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return ""
        return self.get_slice(self.word_base(i) + WORD_IDS)

    def word_strokes(self, char: str) -> str:
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return ""
        return self.get_slice(self.word_base(i) + WORD_STROKES)

    def strip_u_plus_annotations(self, ids: str) -> str:
        """Remove (U+XXXX) / (u+xxxx) annotations; keep the preceding Han leaf."""
        if not ids:
            return ids
        out = []
        i = 0
        n = len(ids)
        while i < n:
            if (
                ids[i] == "("
                and i + 2 < n
                and ids[i + 1] in ("U", "u")
                and ids[i + 2] == "+"
            ):
                j = i + 3
                while j < n and ids[j] != ")":
                    j += 1
                if j < n and ids[j] == ")":
                    i = j + 1
                    continue
            out.append(ids[i])
            i += 1
        return "".join(out)

    def effective_ids(self, char: str) -> str:
        return self.strip_u_plus_annotations(self.word_ids_line(char))

    def word_exist(self, char: str) -> bool:
        self.ensure_index()
        return char in self._word_i

    def seq_level_raw(self, char: str):
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return None
        b = self.word_base(i)
        if self.buf[b + WORD_HAS_SEQ_LEVEL] == 0:
            return None
        return self.buf[b + WORD_SEQ_LEVEL]

    def split_index(self, char: str) -> int:
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return 0
        return self.buf[self.word_base(i) + WORD_SPLIT_INDEX]

    def set_seq_level(self, char: str, lvl) -> None:
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return
        b = self.word_base(i)
        if lvl is None:
            self.buf[b + WORD_HAS_SEQ_LEVEL] = 0
            self.buf[b + WORD_SEQ_LEVEL] = 0
        else:
            self.buf[b + WORD_HAS_SEQ_LEVEL] = 1
            self.buf[b + WORD_SEQ_LEVEL] = int(lvl) & 0xFF

    def set_split_index(self, char: str, si) -> None:
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return
        self.buf[self.word_base(i) + WORD_SPLIT_INDEX] = 0 if si is None else int(si) & 0xFF

    def component_encoding(self, key: str) -> str:
        self.ensure_index()
        i = self._comp_i.get(key)
        if i is None:
            return ""
        return self.get_slice(self.comp_base(i) + COMP_ENCODING)

    def set_component_encoding(self, key: str, enc: str) -> None:
        self.ensure_index()
        i = self._comp_i.get(key)
        if i is None:
            return
        self.set_slice(self.comp_base(i) + COMP_ENCODING, enc)

    def word_encoding(self, char: str) -> str:
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return ""
        return self.get_slice(self.word_base(i) + WORD_ENCODING)

    def set_word_enc_post(self, char: str, enc: str, post: str) -> None:
        self.ensure_index()
        i = self._word_i.get(char)
        if i is None:
            return
        b = self.word_base(i)
        self.set_slice(b + WORD_ENCODING, enc)
        self.set_slice(b + WORD_POST_ENCODING, post)

    def iter_word_chars(self):
        self.ensure_index()
        for i in range(self.n_words()):
            yield self.get_slice(self.word_base(i) + WORD_CHAR)

    def wc_rows(self, char: str):
        self.ensure_index()
        rows = []
        for i in self._wc.get(char, []):
            b = OFF_WORD_COMP + i * STRIDE_WORD_COMP
            rows.append(
                (self.buf[b + EDGE_SEQ], self.get_slice(b + EDGE_TO))
            )
        rows.sort(key=lambda r: r[0])
        return rows

    def wc_comp_keys_str(self, char: str) -> str:
        return "".join(to for _seq, to in self.wc_rows(char))

    def wi_rows(self, char: str):
        self.ensure_index()
        rows = []
        for i in self._wi.get(char, []):
            b = OFF_WORD_IDS + i * STRIDE_WORD_IDS
            rows.append(
                (self.buf[b + WIDS_SEQ], self.buf[b + WIDS_LEAF_INDEX])
            )
        rows.sort(key=lambda r: r[0])
        return rows

    def ws_rows(self, char: str):
        self.ensure_index()
        rows = []
        for i in self._ws.get(char, []):
            b = OFF_WORD_STROKE + i * STRIDE_WORD_STROKE
            rows.append(
                (self.buf[b + WSTR_SEQ], self.get_slice(b + WSTR_TO))
            )
        rows.sort(key=lambda r: r[0])
        return rows

    def override_of(self, char: str) -> tuple[str, str]:
        self.ensure_index()
        return self._ovr.get(char, ("", ""))

    def entity_component_of(self, entity: str) -> str:
        self.ensure_index()
        return self._ent.get(entity, "")

    def _add_comp(self, ch, cns, uni, enc=""):
        i = u64(self.buf, OFF_N_COMPS)
        if i >= CAP_COMPONENTS:
            raise MemoryError("components full")
        b = self.comp_base(i)
        self.set_slice(b + COMP_CHAR, ch)
        self.set_slice(b + COMP_CNS, cns)
        self.set_slice(b + COMP_UNICODE, uni)
        if enc:
            self.set_slice(b + COMP_ENCODING, enc)
        set_u64(self.buf, OFF_N_COMPS, i + 1)

    def _add_word(self, ch, cns, strokes, ids):
        i = u64(self.buf, OFF_N_WORDS)
        if i >= CAP_WORDS:
            raise MemoryError("words full")
        b = self.word_base(i)
        self.set_slice(b + WORD_CHAR, ch)
        self.set_slice(b + WORD_CNS, cns)
        self.set_slice(b + WORD_STROKES, strokes)
        self.set_slice(b + WORD_IDS, ids)
        set_u64(self.buf, OFF_N_WORDS, i + 1)

    def _add_wi(self, word, seq, leaf):
        i = u64(self.buf, OFF_N_WORD_IDS)
        if i >= CAP_WORD_IDS:
            raise MemoryError("word_ids full")
        b = OFF_WORD_IDS + i * STRIDE_WORD_IDS
        self.set_slice(b + WIDS_FROM, word)
        self.buf[b + WIDS_SEQ] = seq & 0xFF
        self.buf[b + WIDS_LEAF_INDEX] = leaf & 0xFF
        set_u64(self.buf, OFF_N_WORD_IDS, i + 1)

    def _add_wc(self, word, seq, comp):
        i = u64(self.buf, OFF_N_WORD_COMP)
        if i >= CAP_WORD_COMP:
            raise MemoryError("word_comp full")
        b = OFF_WORD_COMP + i * STRIDE_WORD_COMP
        self.set_slice(b + EDGE_FROM, word)
        self.set_slice(b + EDGE_TO, comp)
        self.buf[b + EDGE_SEQ] = seq & 0xFF
        set_u64(self.buf, OFF_N_WORD_COMP, i + 1)

    def _add_ws(self, word, seq, digit):
        i = u64(self.buf, OFF_N_WORD_STROKE)
        if i >= CAP_WORD_STROKE:
            raise MemoryError("word_stroke full")
        b = OFF_WORD_STROKE + i * STRIDE_WORD_STROKE
        self.set_slice(b + WSTR_FROM, word)
        self.set_slice(b + WSTR_TO, digit)
        self.buf[b + WSTR_SEQ] = seq & 0xFF
        set_u64(self.buf, OFF_N_WORD_STROKE, i + 1)

    def _add_ent(self, entity, comp):
        i = u64(self.buf, OFF_N_IDS_ENTITY)
        if i >= CAP_IDS_ENTITY:
            raise MemoryError("entity full")
        b = OFF_IDS_ENTITY + i * STRIDE_IDS_ENTITY
        self.set_slice(b + ENT_ENTITY, entity)
        self.set_slice(b + ENT_COMPONENT, comp)
        set_u64(self.buf, OFF_N_IDS_ENTITY, i + 1)

    def _add_ovr(self, ch, enc, post):
        i = u64(self.buf, OFF_N_OVERRIDE)
        if i >= CAP_OVERRIDES:
            raise MemoryError("override full")
        b = OFF_OVERRIDES + i * STRIDE_OVERRIDES
        self.set_slice(b + OVR_CHAR, ch)
        self.set_slice(b + OVR_ENCODING, enc)
        self.set_slice(b + OVR_POST, post)
        set_u64(self.buf, OFF_N_OVERRIDE, i + 1)

    def _add_stroke_type(self, code, name, glyph):
        i = u64(self.buf, OFF_N_STROKE_TYPES)
        b = OFF_STROKE_TYPES + i * STRIDE_STROKE_TYPES
        set_u64(self.buf, b, code)
        self.set_slice(b + 8, name)
        self.set_slice(b + 24, glyph)
        set_u64(self.buf, OFF_N_STROKE_TYPES, i + 1)

    def import_all(self) -> None:
        if self.n_words() > 0:
            return
        self._add_stroke_type(1, "橫", "㇐")
        self._add_stroke_type(2, "豎", "丨")
        self._add_stroke_type(3, "撇", "丿")
        self._add_stroke_type(4, "點", "丶")
        self._add_stroke_type(5, "折", "㇕")
        self._import_tsv_comps()
        self._import_tsv_words()
        self._import_tsv_wi()
        self._import_tsv_wc()
        self._import_tsv_ws()
        self._import_tsv_ent()
        self._import_luna_marks()
        self._import_overrides()
        self._import_locked_enc()
        self.reindex()

    def _read_tsv(self, name: str):
        path = os.path.join(DATA_DIR, name)
        with open(path, "r", encoding="utf-8") as f:
            lines = f.read().split("\n")
        skip = True
        for line in lines:
            line = line.rstrip("\r")
            if not line:
                continue
            if skip:
                skip = False
                continue
            yield [c.rstrip("\r") for c in line.split("\t")]

    def _import_tsv_comps(self):
        for cells in self._read_tsv("components.tsv"):
            ch = cells[0].strip() if cells else ""
            if not ch:
                continue
            cns = cells[1].strip() if len(cells) > 1 else ""
            uni = cells[2].strip() if len(cells) > 2 else ""
            self._add_comp(ch, cns, uni)

    def _import_tsv_words(self):
        for cells in self._read_tsv("words.tsv"):
            ch = cells[0].strip() if cells else ""
            if not ch:
                continue
            cns = cells[1].strip() if len(cells) > 1 else ""
            strokes = cells[2].strip() if len(cells) > 2 else ""
            ids = cells[3].strip() if len(cells) > 3 else ""
            self._add_word(ch, cns, strokes, ids)

    def _import_tsv_wi(self):
        for cells in self._read_tsv("word_ids.tsv"):
            word = cells[0].strip()
            seq = int(cells[1].strip())
            leaf = int(cells[2].strip())
            self._add_wi(word, seq, leaf)

    def _import_tsv_wc(self):
        for cells in self._read_tsv("word_components.tsv"):
            word = cells[0].strip()
            seq = int(cells[1].strip())
            comp = cells[2].strip() if len(cells) > 2 else ""
            self._add_wc(word, seq, comp)

    def _import_tsv_ws(self):
        for cells in self._read_tsv("word_stroke.tsv"):
            word = cells[0].strip()
            seq = int(cells[1].strip())
            digit = cells[2].strip() if len(cells) > 2 else ""
            self._add_ws(word, seq, digit)

    def _import_tsv_ent(self):
        for cells in self._read_tsv("ids_entity.tsv"):
            self._add_ent(cells[0].strip(), cells[1].strip() if len(cells) > 1 else "")

    def _import_luna_marks(self):
        # Index not built yet; scan words by char via linear after words filled.
        tmp = {}
        for i in range(self.n_words()):
            tmp[self.get_slice(self.word_base(i) + WORD_CHAR)] = i
        for cells in self._read_tsv("words.tsv"):
            ch = cells[0].strip()
            i = tmp.get(ch)
            if i is None:
                continue
            seq_lvl = cells[4].strip() if len(cells) > 4 else ""
            split_idx = cells[5].strip() if len(cells) > 5 else ""
            b = self.word_base(i)
            if seq_lvl:
                self.buf[b + WORD_HAS_SEQ_LEVEL] = 1
                self.buf[b + WORD_SEQ_LEVEL] = int(seq_lvl) & 0xFF
            if split_idx:
                self.buf[b + WORD_SPLIT_INDEX] = int(split_idx) & 0xFF

    def _import_overrides(self):
        for cells in self._read_tsv("luna_word_override.tsv"):
            word = cells[0].strip()
            enc = cells[1].strip() if len(cells) > 1 else ""
            post = cells[2].strip() if len(cells) > 2 else ""
            self._add_ovr(word, enc, post)

    def _import_locked_enc(self):
        tmp = {}
        for i in range(self.n_comps()):
            tmp[self.get_slice(self.comp_base(i) + COMP_CHAR)] = i
        for cells in self._read_tsv("component_encoding.tsv"):
            ch = cells[0].strip()
            enc = cells[2].strip() if len(cells) > 2 else ""
            i = tmp.get(ch)
            if i is None or not enc:
                continue
            self.set_slice(self.comp_base(i) + COMP_ENCODING, enc)

    def is_idc(self, c: str) -> bool:
        return c in IDC

    def is_entity_start(self, c: str) -> bool:
        return c == "&"

    def entity_end(self, ids: str, idx: int) -> int:
        pos = idx + 1
        while pos < len(ids) and ids[pos] != ";":
            pos += 1
        if pos < len(ids) and ids[pos] == ";":
            return pos + 1
        return pos

    def is_entity_leaf(self, leaf: str) -> bool:
        return len(leaf) > 0 and leaf[0] == "&"

    def strip_idc_leaves(self, ids: str) -> str:
        out = []
        idx = 0
        while idx < len(ids):
            c = ids[idx]
            if self.is_idc(c):
                idx += 1
                continue
            if self.is_entity_start(c):
                end = self.entity_end(ids, idx)
                tok = ids[idx:end]
                out.append(tok)
                idx = end
                continue
            out.append(c)
            idx += 1
        return "§".join(out)

    def strip_idc_chars(self, s: str) -> str:
        return "".join(c for c in s if not self.is_idc(c))

    def ids_has_entity(self, ids_line: str) -> bool:
        return "&" in ids_line

    def ids_leaves_str(self, char: str) -> str:
        ids_line = self.effective_ids(char)
        if self.ids_has_entity(ids_line):
            return self.strip_idc_leaves(ids_line)
        return self.strip_idc_chars(ids_line)

    def ids_leaf_count(self, char: str) -> int:
        leaves = self.ids_leaves_str(char)
        ids_line = self.effective_ids(char)
        if self.ids_has_entity(ids_line):
            if not leaves:
                return 0
            return len(leaves.split("§"))
        return len(leaves)

    def ids_leaf_at(self, char: str, idx: int) -> str:
        leaves = self.ids_leaves_str(char)
        ids_line = self.effective_ids(char)
        if self.ids_has_entity(ids_line):
            if not leaves:
                return ""
            parts = leaves.split("§")
            return parts[idx] if 0 <= idx < len(parts) else ""
        if 0 <= idx < len(leaves):
            return leaves[idx]
        return ""

    def leaf_tokens_in_ids_str(self, unit_ids: str) -> str:
        unit_ids = self.strip_u_plus_annotations(unit_ids)
        if self.ids_has_entity(unit_ids):
            return self.strip_idc_leaves(unit_ids)
        return self.strip_idc_chars(unit_ids)

    def leaf_token_count(self, leaves_str: str) -> int:
        if "§" in leaves_str:
            return 0 if not leaves_str else len(leaves_str.split("§"))
        return len(leaves_str)

    def leaf_token_at(self, leaves_str: str, idx: int) -> str:
        if "§" in leaves_str:
            parts = leaves_str.split("§")
            return parts[idx] if 0 <= idx < len(parts) else ""
        if 0 <= idx < len(leaves_str):
            return leaves_str[idx]
        return ""

    def ids_line_impure(self, ids_line: str) -> bool:
        if not ids_line:
            return True
        if "〾" in ids_line or "&" in ids_line:
            return True
        return False

    def default_sequence_level(self, char: str) -> int:
        ids_line = self.effective_ids(char)
        if self.ids_line_impure(ids_line):
            if self.wc_comp_keys_str(char):
                return 3
        return 1

    def word_sequence_level(self, char: str) -> int:
        if not self.word_exist(char):
            return self.default_sequence_level(char)
        lvl = self.seq_level_raw(char)
        if lvl is None:
            return self.default_sequence_level(char)
        return int(lvl)

    def comp_keys_str_for_level(self, char: str, level: int) -> str:
        if level == 3:
            return self.wc_comp_keys_str(char)
        if level == 2:
            perm_str = self.components_of_ids_perm(char)
            return perm_str if perm_str else self.components_of_ids_formula(char)
        formula = self.components_of_ids_formula(char)
        return formula if formula else self.wc_comp_keys_str(char)

    def components_of_ids_formula(self, char: str) -> str:
        n = self.ids_leaf_count(char)
        if n <= 1:
            return ""
        out = []
        for idx in range(n):
            leaf = self.ids_leaf_at(char, idx)
            out.append(self.components_of(leaf))
        return "".join(out)

    def components_of_ids_perm(self, char: str) -> str:
        n = self.ids_leaf_count(char)
        if n <= 1:
            return ""
        perm = self.wi_rows(char)
        if not perm:
            return ""
        out = []
        for _seq, leaf_index in perm:
            leaf = self.ids_leaf_at(char, leaf_index - 1)
            out.append(self.components_of(leaf))
        return "".join(out)

    def components_of_general(self, char: str) -> str:
        perm_str = self.components_of_ids_perm(char)
        if perm_str:
            return perm_str
        return self.components_of_ids_formula(char)

    def components_of(self, char: str) -> str:
        if self.is_entity_leaf(char):
            return self.entity_component_of(char)
        return self.comp_keys_str_for_level(char, self.word_sequence_level(char))

    def derived_strokes_general(self, char: str) -> str:
        n = self.ids_leaf_count(char)
        if n <= 1:
            return self.word_strokes(char)
        perm = self.wi_rows(char)
        if perm:
            out = []
            for _seq, li in perm:
                leaf = self.ids_leaf_at(char, li - 1)
                out.append(self.derived_strokes(leaf))
            return "".join(out)
        out = []
        for idx in range(n):
            leaf = self.ids_leaf_at(char, idx)
            out.append(self.derived_strokes(leaf))
        return "".join(out)

    def derived_strokes(self, char: str) -> str:
        if self.word_sequence_level(char) == 3:
            return self.word_strokes(char)
        return self.derived_strokes_general(char)

    def stroke_order(self, char: str) -> str:
        ov = self.ws_rows(char)
        if ov:
            return "".join(digit for _seq, digit in ov)
        return self.derived_strokes(char) or ""

    def smoke(self) -> int:
        chk = (
            self.n_words()
            + self.n_comps()
            + u64(self.buf, OFF_N_WORD_IDS)
            + u64(self.buf, OFF_N_WORD_COMP)
            + u64(self.buf, OFF_N_WORD_STROKE)
        )
        set_u64(self.buf, OFF_SMOKE_CHK, chk)
        return chk


def import_all(view) -> None:
    World(view).import_all()


def components_of(view) -> None:
    w = World(view)
    w.write_result(w.components_of(w.read_query()))


def stroke_order(view) -> None:
    w = World(view)
    w.write_result(w.stroke_order(w.read_query()))


def smoke(view) -> None:
    World(view).smoke()
