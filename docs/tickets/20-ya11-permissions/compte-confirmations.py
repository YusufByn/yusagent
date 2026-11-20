import glob, json, re, collections, shlex, statistics, sys

# Sessions principales des runs réels du smoke bench (hors capture, chauffe, sonde, sous-agents).
PATHS = glob.glob("bench/smoke/runs/*/tasks/*/run-*/sessions/*.jsonl") + glob.glob(
    "bench/smoke/runs/*/arms/*/tasks/*/run-*/sessions/*.jsonl")

STR = r"""(?P<p>[rRfFbB]{0,2})(?P<q>'''|\"\"\"|'|")(?P<s>.*?)(?P=q)"""
BASH_CALL = re.compile(r"\bbash\(\s*")
BASH_LIT = re.compile(STR, re.S)
EDIT_CALL = re.compile(r"(?<![\w.])edit\(")
PY_WRITE = re.compile(
    r"open\([^)]*['\"][rbt]*[wax+]|\.write_text\(|\.write_bytes\(|\bos\.(remove|unlink|rename|replace|"
    r"makedirs|mkdir|rmdir|system|popen|chmod|symlink|link)\b|\.unlink\(|\.mkdir\(|\.rename\(|"
    r"\.touch\(|\bshutil\.|\bsubprocess\b|\bsocket\b|\burllib\b|\brequests\.|\bhttp\.client\b")
OPS = {";", "&&", "||", "|", "&", "|&", ";;"}


def segments(cmd: str):
    """Commandes simples d'une ligne shell (guillemets respectés), None si illisible ou redirection
    vers un fichier."""
    try:
        lex = shlex.shlex(cmd.replace("\n", ";"), posix=True, punctuation_chars=True)
        lex.whitespace_split = True
        tokens = list(lex)
    except ValueError:
        return None
    out, cur = [], []
    for i, t in enumerate(tokens):
        if t in OPS:
            out.append(cur); cur = []
        elif t in (">", ">>", "&>", ">|"):
            if i + 1 >= len(tokens) or tokens[i + 1] != "/dev/null":
                return None
        else:
            cur.append(t)
    out.append(cur)
    return [c for c in out if c]


RO_PROGS = {"ls", "cat", "head", "tail", "wc", "grep", "egrep", "rg", "find", "sed", "awk", "sort",
            "uniq", "cut", "tr", "nl", "echo", "printf", "pwd", "which", "type", "file", "stat", "du",
            "tree", "diff", "cmp", "basename", "dirname", "realpath", "readlink", "true", "test",
            "jq", "column", "xargs"}
RO_GIT = {"status", "diff", "log", "show", "grep", "ls-files", "rev-parse", "blame", "describe"}


def segment_ro(words: list) -> bool:
    words = [w for w in words if w not in ("(", ")", "{", "}")]
    while words and re.match(r"^\w+=", words[0]):
        words = words[1:]
    if not words:
        return True
    prog = words[0].split("/")[-1]
    if prog == "git":
        sub = next((w for w in words[1:] if not w.startswith("-")), "")
        return sub in RO_GIT
    if prog in ("2", "1") or "--version" in words or "-v" in words[1:2]:
        return True
    if prog == "sed":
        return not any(w.startswith("-i") for w in words) and "-n" in words
    if prog == "find":
        return not any(w in ("-delete", "-exec", "-execdir", "-ok", "-fprint") for w in words)
    if prog == "xargs":
        return segment_ro(words[1:])
    return prog in RO_PROGS


def command_ro(cmd: str) -> bool:
    segs = segments(cmd)
    return segs is not None and all(segment_ro(w) for w in segs)


def key(cmd: str) -> str:
    segs = segments(cmd) or [cmd.split()]
    words = [w for w in segs[0] if not re.match(r"^\w+=", w)]
    runner = words[:1] in (["npx"], ["uvx"]) or words[:2] in (["uv", "run"], ["npm", "run"]) or (
        len(words) > 1 and words[1] == "-m")
    return " ".join(words[:3] if runner else words[:2])


def bash_calls(code: str):
    """(littéral ou None, est une f-string) pour chaque appel bash(...) du texte de la cellule."""
    for m in BASH_CALL.finditer(code):
        lit = BASH_LIT.match(code, m.end())
        if lit and "f" not in lit.group("p").lower():
            yield lit.group("s")
        else:
            yield None


mode_cache = {}
stats = collections.defaultdict(collections.Counter)
per_session = collections.defaultdict(lambda: collections.defaultdict(list))
for path in PATHS:
    run = path.split("/tasks/")[0]
    if run not in mode_cache:
        try:
            mode_cache[run] = json.load(open(run + "/report.json"))["header"].get("mode")
        except Exception:
            mode_cache[run] = None
    if mode_cache[run] != "réel":
        continue
    lines = open(path).read().splitlines()
    head = json.loads(lines[0]) if lines else {}
    if head.get("type") != "session":
        continue
    v = f"v{(head.get('yaRequest') or {}).get('promptVersion', 1)}"
    s = stats[v]
    s["sessions"] += 1
    c = collections.Counter()
    keys_c = set()
    approved_b = set()  # (b) + « toujours pour cette commande » dans la session
    for line in lines[1:]:
        m = json.loads(line).get("message") or {}
        if m.get("role") != "assistant":
            continue
        for block in m.get("content", []):
            if block.get("type") != "toolCall":
                continue
            name = block.get("name")
            c["a_tool_calls"] += 1
            if name != "python_repl":
                c["other_tools_" + name] += 1
                # apply_patch / exec_command : une confirmation par appel dans (b) et (c)
                c["b_confirm"] += 1
                c["c_confirm"] += 1
                c["c2_confirm"] += 1
                keys_c.add(name)
                if name not in approved_b:
                    c["b2_confirm"] += 1
                    approved_b.add(name)
                continue
            code = (block.get("arguments") or {}).get("code") or ""
            c["cells"] += 1
            calls = list(bash_calls(code))
            edits = len(EDIT_CALL.findall(code))
            pyw = len(PY_WRITE.findall(code))
            dyn = sum(1 for x in calls if x is None)
            ro = [x for x in calls if x is not None and command_ro(x)]
            nro = [x for x in calls if x is not None and not command_ro(x)]
            c["bash_calls"] += len(calls)
            c["bash_dynamic"] += dyn
            c["bash_ro"] += len(ro)
            c["edit_calls"] += edits
            c["cells_py_write"] += bool(pyw)
            c["py_write"] += pyw
            cell_ro = not calls or (dyn == 0 and not nro)
            cell_ro = cell_ro and edits == 0 and pyw == 0
            c["cells_ro"] += cell_ro
            c["b_confirm"] += not cell_ro
            # (c) : chaque bash() et chaque écriture (edit() + motif d'écriture Python)
            c["c_confirm"] += len(calls) + edits + pyw
            # (c') : bash() en lecture seule auto-approuvés
            c["c2_confirm"] += dyn + len(nro) + edits + pyw
            for x in nro:
                keys_c.add(key(x))
            if dyn:
                keys_c.add("<dynamique>")
            if edits:
                keys_c.add("edit()")
            if pyw:
                keys_c.add("<écriture python>")
            # (b) + « toujours » : motifs mémorisables (commande, edit()) ; bash() dynamique et
            # écriture Python jamais mémorisés.
            reasons = {key(x) for x in nro} | ({"edit()"} if edits else set())
            if not cell_ro and (dyn or pyw or reasons - approved_b):
                c["b2_confirm"] += 1
                approved_b |= reasons
    # (c') + « toujours » : une fois par motif mémorisable, à chaque fois pour le reste
    c["c3_confirm"] = len(keys_c - {"<dynamique>", "<écriture python>"}) + c["bash_dynamic"] + c["py_write"]
    for k, n in c.items():
        s[k] += n
        per_session[v][k].append(n)

for v in sorted(stats):
    s = stats[v]
    print(v, dict(sorted(s.items())))
    for k in ("a_tool_calls", "cells", "b_confirm", "b2_confirm", "c_confirm", "c2_confirm", "c3_confirm"):
        xs = sorted(per_session[v][k] + [0] * (s["sessions"] - len(per_session[v][k])))
        print(f"  {k}: total {sum(xs)}, médiane {statistics.median(xs)}, p90 {xs[int(0.9 * (len(xs) - 1))]}, max {xs[-1]}")
