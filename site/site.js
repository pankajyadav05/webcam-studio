// Webcam Studio landing page: background chips, before/after line, framing demo, menu switch.
document.documentElement.classList.add("js");

// The app's six drawn backgrounds (apps/desktop/ui/lib/backgrounds.js), same colours.
const PRESETS = {
  studio: { a: "#41444c", b: "#17181b", glow: "rgba(255,255,255,0.16)" },
  paper: { a: "#f1ebe1", b: "#d6cdbf", glow: "rgba(255,255,255,0.55)" },
  sage: { a: "#93a99a", b: "#566b5f", glow: "rgba(255,255,255,0.24)" },
  dusk: { a: "#3f6072", b: "#1b2933", glow: "rgba(255,190,140,0.2)" },
  sand: { a: "#dcbd98", b: "#a98260", glow: "rgba(255,255,255,0.32)" },
  slate: { a: "#707684", b: "#333842", glow: "rgba(255,255,255,0.18)" },
};
const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;

function paint(el, id) {
  const p = PRESETS[id];
  el.classList.add("preset");
  el.style.setProperty("--a", p.a);
  el.style.setProperty("--b", p.b);
  el.style.setProperty("--glow", p.glow);
}

// Hero: chips work like a colour picker for the background.
const stage = document.querySelector("[data-stage]");
if (stage) {
  const frame = stage.querySelector(".stage-frame");
  const preset = stage.querySelector(".stage-preset");
  const name = stage.querySelector("[data-bg-name]");
  const chips = [...stage.querySelectorAll(".chip")];

  const select = (chip, focus = false) => {
    const bg = chip.dataset.bg;
    chips.forEach((c) => {
      c.setAttribute("aria-checked", String(c === chip));
      c.tabIndex = c === chip ? 0 : -1;
    });
    frame.dataset.bg = bg;
    frame.classList.toggle("is-preset", bg in PRESETS);
    if (bg in PRESETS) paint(preset, bg);
    name.textContent = chip.dataset.name;
    if (focus) chip.focus();
  };

  chips.forEach((chip, i) => {
    chip.addEventListener("click", () => select(chip));
    chip.addEventListener("keydown", (e) => {
      const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
      if (!step) return;
      e.preventDefault();
      select(chips[(i + step + chips.length) % chips.length], true);
    });
  });
  select(chips.find((c) => c.getAttribute("aria-checked") === "true") || chips[0]);
}

// Replace panel: the six backgrounds turn slowly behind the person.
const cycle = document.querySelector("[data-cycle]");
if (cycle) {
  const ids = Object.keys(PRESETS);
  let n = 0;
  paint(cycle, ids[n]);
  if (!reduceMotion) {
    setInterval(() => {
      if (document.hidden) return;
      n = (n + 1) % ids.length;
      cycle.style.opacity = "0";
      setTimeout(() => {
        paint(cycle, ids[n]);
        cycle.style.opacity = "1";
      }, 400);
    }, 2600);
  }
}

// Before and after: a range input drives the line, so keyboards and touch work too.
document.querySelectorAll("[data-compare]").forEach((box) => {
  const range = box.querySelector(".compare-range");
  const set = () => box.style.setProperty("--pos", `${range.value}%`);
  range.addEventListener("input", set);
  set();
});

// Framing: zoom steps and mirror, as in the app.
const framing = document.querySelector("[data-frame]");
if (framing) {
  const img = framing.querySelector(".frame-view img");
  const out = framing.querySelector("[data-zoom-value]");
  const out_ = framing.querySelectorAll("[data-zoom]");
  const mirror = framing.querySelector("[data-mirror]");
  const steps = [1, 1.25, 1.5, 1.75, 2];
  let at = 0;
  const render = () => {
    img.style.setProperty("--zoom", steps[at]);
    out.textContent = `${String(Number(steps[at].toFixed(2)))}×`.replace(/^(\d)×$/, "$1.0×");
    out_.forEach((b) => (b.disabled = (b.dataset.zoom === "-1" && at === 0) || (b.dataset.zoom === "1" && at === steps.length - 1)));
  };
  out_.forEach((b) => b.addEventListener("click", () => {
    at = Math.min(steps.length - 1, Math.max(0, at + Number(b.dataset.zoom)));
    render();
  }));
  mirror.addEventListener("click", () => {
    const on = mirror.getAttribute("aria-pressed") !== "true";
    mirror.setAttribute("aria-pressed", String(on));
    img.style.setProperty("--flip", on ? -1 : 1);
  });
  render();
}

// Menu bar panel: the switch changes the status, as in the app. The panel hangs under the icon.
const menu = document.querySelector("[data-menu]");
if (menu) {
  const toggle = menu.querySelector("[data-menu-switch]");
  const status = menu.querySelector("[data-menu-status]");
  toggle.addEventListener("change", () => {
    menu.classList.toggle("is-off", !toggle.checked);
    menu.closest("[data-mac]")?.classList.toggle("is-off", !toggle.checked);
    status.textContent = toggle.checked ? "In use · 30 fps" : "Off";
  });
  const mac = document.querySelector("[data-mac]");
  const icon = mac.querySelector("[data-mac-icon]");
  const place = () => {
    const box = mac.getBoundingClientRect();
    const at = icon.getBoundingClientRect();
    const centre = at.left + at.width / 2 - box.left;
    const left = Math.min(Math.max(8, centre - menu.offsetWidth / 2), box.width - menu.offsetWidth - 8);
    menu.style.left = `${left}px`;
    menu.style.right = "auto";
  };
  place();
  addEventListener("resize", place);
  document.fonts?.ready.then(place);
}

// Product bar: mark the section on screen.
const links = [...document.querySelectorAll(".bar-nav a")];
const targets = links.map((a) => document.querySelector(a.getAttribute("href"))).filter(Boolean);
if ("IntersectionObserver" in window && targets.length) {
  const seen = new Map();
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => seen.set(e.target, e.isIntersecting));
    const current = targets.find((t) => seen.get(t));
    links.forEach((a) => a.setAttribute("aria-current", String(current && a.getAttribute("href") === `#${current.id}`)));
  }, { rootMargin: "-45% 0px -50% 0px" });
  targets.forEach((t) => io.observe(t));
}
