import ast, glob, warnings, json, re, collections, shlex, statistics, sys

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



# --- D2 strict (décision de Yusuf du 20/11/2026) : liste blanche sur l'AST, refus par défaut ---
STRICT_CALLS = {"read", "grep", "glob", "print", "bash"}  # grep() et glob() n'existent pas encore
STRICT_ATTRS = {"output", "exit_code"}
STRICT_NODES = (ast.Module, ast.Expr, ast.Assign, ast.AnnAssign, ast.AugAssign, ast.Name, ast.Load,
                ast.Store, ast.Constant, ast.JoinedStr, ast.FormattedValue, ast.Attribute,
                ast.Subscript, ast.Slice, ast.Tuple, ast.List, ast.Dict, ast.Set, ast.BinOp,
                ast.UnaryOp, ast.BoolOp, ast.Compare, ast.operator, ast.unaryop, ast.boolop,
                ast.cmpop, ast.Await, ast.Call, ast.keyword)
SHELL_OPS = (";", "&", "|", "$(", "`", ">", "<", "\n")
SAFE_PROGS = {"ls", "cat", "head", "tail", "wc", "grep", "rg", "find", "sed", "git", "pwd", "tree",
              "stat", "file", "diff"}
SAFE_GIT = {"status", "diff", "log", "show", "ls-files", "rev-parse", "blame", "grep"}
EXEC_OPTS = {
    "rg": ("--pre", "--pre-glob", "-z", "--search-zip"),
    "find": ("-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls"),
    "git": ("-c", "--config-env", "--exec-path", "--output", "--ext-diff", "-O", "--open-files-in-pager"),
    "tree": ("-o",),
    "file": ("-C", "--compile"),
}
SED_PRINT = re.compile(r"^[0-9$]+(,[0-9$]+)?p$")


def has_shell_op(cmd: str) -> bool:
    return any(op in cmd for op in SHELL_OPS)


def strict_safe_command(cmd: str) -> bool:
    """Lecture sûre listée : une seule commande simple, sans opérateur ni option qui exécute."""
    if has_shell_op(cmd):
        return False
    try:
        words = shlex.split(cmd)
    except ValueError:
        return False
    if not words or "=" in words[0] or words[0] not in SAFE_PROGS:
        return False
    prog = words[0]
    if any(w.split("=")[0] in EXEC_OPTS.get(prog, ()) for w in words[1:]):
        return False
    if prog == "git":
        sub = [w for w in words[1:] if not w.startswith("-")]
        return bool(sub) and sub[0] in SAFE_GIT and all(w == "--no-pager" for w in words[1:words.index(sub[0])])
    if prog == "sed":
        args = [w for w in words[1:] if not w.startswith("-")]
        return words[1:].count("-n") == 1 and all(w == "-n" for w in words[1:] if w.startswith("-")) \
            and bool(args) and SED_PRINT.match(args[0]) is not None
    return True


def strict_cell(code: str):
    """(structure dans la liste blanche, commandes bash() littérales non sûres, toutes les commandes
    littérales). Structure fausse : cellule illisible, nœud ou appel hors liste, bash() non littéral."""
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", SyntaxWarning)
            tree = ast.parse(code)
    except SyntaxError:
        return False, [], []
    ok, unsafe, literals = True, [], []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == "bash":
            arg = node.args[0] if node.args else None
            if isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                literals.append(arg.value)
                if not strict_safe_command(arg.value):
                    unsafe.append(arg.value)
            else:
                ok = False
            if len(node.args) > 1 or any(k.arg != "timeout" for k in node.keywords):
                ok = False
        if not isinstance(node, STRICT_NODES):
            ok = False
        elif isinstance(node, ast.Call) and not (isinstance(node.func, ast.Name) and node.func.id in STRICT_CALLS):
            ok = False
        elif isinstance(node, ast.Call) and node.func.id in ("print", "read") and any(k.arg == "file" for k in node.keywords):
            ok = False
        elif isinstance(node, ast.Attribute) and node.attr not in STRICT_ATTRS:
            ok = False
        elif isinstance(node, ast.Name) and node.id.startswith("__"):
            ok = False
    return ok, unsafe, literals


def strict_reasons(code: str, unsafe: list) -> set:
    """Raisons d'une confirmation en D2 strict (une cellule peut en avoir plusieurs)."""
    out = set()
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", SyntaxWarning)
            tree = ast.parse(code)
    except SyntaxError:
        return {"illisible"}
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            name = node.func.id if isinstance(node.func, ast.Name) else None
            if name == "edit":
                out.add("edit()")
            elif name == "bash":
                arg = node.args[0] if node.args else None
                if not (isinstance(arg, ast.Constant) and isinstance(arg.value, str)):
                    out.add("bash() non littéral")
            elif name not in STRICT_CALLS:
                out.add("autre appel")
        elif not isinstance(node, STRICT_NODES):
            out.add("autre construction (import, boucle, compréhension…)")
    for x in unsafe:
        out.add("bash() avec opérateur" if has_shell_op(x) else "bash() sans opérateur, hors liste")
    return out


def ops_safe_command(cmd: str) -> bool:
    """Sensibilité seulement (pas une option de D2) : `;`, `&&`, `||`, `|` permis entre lectures sûres."""
    if any(op in cmd for op in ("$(", "`", "<")):
        return False
    segs = segments(cmd)
    return segs is not None and all(strict_safe_command(" ".join(shlex.quote(w) for w in seg)) for seg in segs)


reasons_v = collections.defaultdict(collections.Counter)
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
    approved_s = set()  # D2 strict + « toujours » : commandes exactes sans opérateur
    memo_seen = collections.Counter()  # commandes sans opérateur hors liste, cellules confirmées
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
                c["s_confirm"] += 1
                c["s2_confirm"] += 1
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
            # D2 strict
            s_ok, s_unsafe, s_literals = strict_cell(code)
            c["cells_strict_ok"] += s_ok and not s_unsafe
            c["cells_strict_ops_ok"] += s_ok and all(ops_safe_command(x) for x in s_unsafe)
            if not (s_ok and not s_unsafe):
                for x in s_unsafe:
                    if not has_shell_op(x):
                        memo_seen[x] += 1
                r = strict_reasons(code, s_unsafe)
                for k in r:
                    reasons_v[v][k] += 1
                    if len(r) == 1:
                        reasons_v[v][k + " [seule raison]"] += 1
            c["s_confirm"] += not (s_ok and not s_unsafe)
            if s_ok and all(x in approved_s for x in s_unsafe):
                c["cells_strict_ok_always"] += 1
            else:
                c["s2_confirm"] += 1
                # majorant : « toujours » choisi à chaque confirmation, pour toutes les commandes
                # exactes sans opérateur de la cellule
                approved_s |= {x for x in s_literals if not has_shell_op(x) and not strict_safe_command(x)}
    c["memo_cmd_occurrences"] = sum(memo_seen.values())
    c["memo_cmd_occurrences_unique_in_session"] = sum(n for n in memo_seen.values() if n == 1)
    # (c') + « toujours » : une fois par motif mémorisable, à chaque fois pour le reste
    c["c3_confirm"] = len(keys_c - {"<dynamique>", "<écriture python>"}) + c["bash_dynamic"] + c["py_write"]
    for k, n in c.items():
        s[k] += n
        per_session[v][k].append(n)

for v in sorted(stats):
    s = stats[v]
    print(v, dict(sorted(s.items())))
    for k in ("a_tool_calls", "cells", "s_confirm", "s2_confirm", "b_confirm", "b2_confirm", "c_confirm", "c2_confirm", "c3_confirm"):
        xs = sorted(per_session[v][k] + [0] * (s["sessions"] - len(per_session[v][k])))
        print(f"  {k}: total {sum(xs)}, médiane {statistics.median(xs)}, p90 {xs[int(0.9 * (len(xs) - 1))]}, max {xs[-1]}")
    print("  raisons D2 strict :", dict(sorted(reasons_v[v].items())))
