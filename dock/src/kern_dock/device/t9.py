"""T9 multi-tap model and planner mirroring the firmware text input module."""

from __future__ import annotations

T9_UPPER, T9_LOWER, T9_NUMBERS, T9_SYMBOLS = "ABC", "abc", "123", "#@!"
T9_CHARSETS = (T9_UPPER, T9_LOWER, T9_NUMBERS, T9_SYMBOLS)
T9_PUNCT = ".,?!'-"
T9_ALPHA = ("ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ")
T9_SYMBOL_GROUPS = ("@#$", "%&*", "()[]", "<>{}", "/\\|", "+-=", "\"'`", " :;_")
T9_MAX_LEN = 32
T9_TICK_MS = 50
T9_CYCLE_TICKS = 15
T9_TAP_MS = 45
T9_GAP_MS = 70


def t9_chars(charset: str, key: int) -> str:
    if charset == T9_NUMBERS:
        return str(key)
    if key == 0:
        return " "
    if key == 1:
        return T9_PUNCT
    if charset == T9_SYMBOLS:
        return T9_SYMBOL_GROUPS[key - 2]
    grp = T9_ALPHA[key - 2]
    return grp if charset == T9_UPPER else grp.lower()


def t9_switch(charset: str, key: str) -> str:
    """Charset transition on Star ('star') or F/#/Side1 ('f')."""
    if key == "star":
        return {T9_UPPER: T9_NUMBERS, T9_LOWER: T9_NUMBERS, T9_NUMBERS: T9_SYMBOLS}.get(charset, T9_UPPER)
    return T9_LOWER if charset == T9_UPPER else T9_UPPER


def _t9_route(src: str, dst: str) -> list[str]:
    """Shortest Star/F key route between charsets (BFS over a 4-node graph)."""
    frontier: list[tuple[str, list[str]]] = [(src, [])]
    seen = {src}
    while frontier:
        cs, path = frontier.pop(0)
        if cs == dst:
            return path
        for k in ("f", "star"):
            nx = t9_switch(cs, k)
            if nx not in seen:
                seen.add(nx)
                frontier.append((nx, path + [k]))
    return []


class T9Model:
    """Bit-exact Python model of the firmware TextInput state machine."""

    def __init__(self, max_len: int = T9_MAX_LEN) -> None:
        self.max_len = max_len
        self.buffer = ""
        self.charset = T9_UPPER
        self.last_key: int | None = None
        self.press_count = 0
        self.last_tick = 0
        self.exit_pressed = False
        self.exit_press_ms = 0
        self.done = False
        self.canceled = False

    def _confirm(self) -> None:
        self.last_key = None
        self.press_count = 0

    def _push(self, ch: str) -> None:
        self._confirm()
        if len(self.buffer) < self.max_len:
            self.buffer += ch

    def feed(self, key: str, state: str, t_ms: int) -> None:
        tick = t_ms // T9_TICK_MS
        if state == "hold":
            if key == "exit":
                self.canceled = True
            elif key == "menu":
                self._confirm()
                self.done = True
            elif key.isdigit():
                self._push(key)
            return
        if state == "release":
            if key == "exit" and self.exit_pressed:
                self.exit_pressed = False
                if t_ms - self.exit_press_ms >= 6 * T9_TICK_MS:
                    self.canceled = True
                elif self.last_key is not None:
                    self._confirm()
                elif self.buffer:
                    self.buffer = self.buffer[:-1]
                else:
                    self.canceled = True
            return
        if key.isdigit():
            k = int(key)
            if self.charset == T9_NUMBERS:
                self._push(key)
            else:
                chars = t9_chars(self.charset, k)
                if self.last_key == k and tick - self.last_tick < T9_CYCLE_TICKS and self.buffer:
                    self.press_count = (self.press_count + 1) % len(chars)
                    self.buffer = self.buffer[:-1] + chars[self.press_count]
                else:
                    self._push(chars[0])
                    self.last_key = k
                    self.press_count = 0
            self.last_tick = tick
        elif key == "exit":
            self.exit_pressed = True
            self.exit_press_ms = t_ms
        elif key in ("star", "f", "hash", "side1"):
            self._confirm()
            self.charset = t9_switch(self.charset, "star" if key == "star" else "f")
        elif key == "menu":
            self._confirm()
            self.done = True


class T9Planner:
    """Incremental PC-text → radio keypad planner emitting minimal multi-tap sequences."""

    def __init__(self) -> None:
        self.reset()

    def reset(self) -> None:
        self.charset = T9_UPPER
        self.pending: int | None = None
        self.length = 0

    @staticmethod
    def _tap(key: str, out: list[tuple[str, str, int]]) -> None:
        out.append((key, "press", T9_TAP_MS))
        out.append((key, "release", T9_GAP_MS))

    def _candidates(self, ch: str) -> list[tuple[str, int, int]]:
        found = []
        for cs in T9_CHARSETS:
            for k in range(10):
                idx = t9_chars(cs, k).find(ch)
                if idx >= 0:
                    found.append((cs, k, idx))
        return found

    def supports(self, ch: str) -> bool:
        return bool(self._candidates(ch))

    def plan_char(self, ch: str) -> list[tuple[str, str, int]]:
        cands = self._candidates(ch)
        if not cands or self.length >= T9_MAX_LEN:
            return []

        def cost(c: tuple[str, int, int]) -> int:
            cs, k, idx = c
            route = _t9_route(self.charset, cs)
            sep = 1 if (not route and cs != T9_NUMBERS and self.pending == k) else 0
            return len(route) + sep + idx + 1

        cs, k, idx = min(cands, key=cost)
        out: list[tuple[str, str, int]] = []
        route = _t9_route(self.charset, cs)
        for sw in route:
            self._tap(sw, out)
        if route:
            self.pending = None
        self.charset = cs
        if cs != T9_NUMBERS and self.pending == k:
            self._tap("exit", out)
        for _ in range(idx + 1):
            self._tap(str(k), out)
        self.pending = None if cs == T9_NUMBERS else k
        self.length += 1
        return out

    def plan_text(self, text: str) -> list[tuple[str, str, int]]:
        out: list[tuple[str, str, int]] = []
        for ch in text:
            out.extend(self.plan_char(ch))
        return out

    def plan_backspace(self) -> list[tuple[str, str, int]]:
        if self.length == 0:
            return []
        out: list[tuple[str, str, int]] = []
        if self.pending is not None:
            self._tap("exit", out)
            self.pending = None
        self._tap("exit", out)
        self.length -= 1
        return out


def text_to_keypad_sequence(text: str) -> list[tuple[str, str, int]]:
    return T9Planner().plan_text(text)


def simulate_text_input(seq: list[tuple[str, str, int]]) -> str:
    model, t = T9Model(), 0
    for key, state, delay in seq:
        model.feed(key, state, t)
        t += delay
    return model.buffer
