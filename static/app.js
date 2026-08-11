const root = document.documentElement;
const themeToggles = [...document.querySelectorAll("[data-theme-toggle]")];
const modes = ["system", "light", "dark"];

function applyTheme(mode) {
  themeToggles.forEach((toggle) => {
    toggle.dataset.mode = mode;
    const label = `Theme: ${mode === "system" ? "match device" : mode}`;
    toggle.setAttribute("aria-label", label);
    toggle.querySelector("[data-tooltip]").textContent = label;
  });

  if (mode === "system") {
    delete root.dataset.theme;
    localStorage.removeItem("theme");
  } else {
    root.dataset.theme = mode;
    localStorage.setItem("theme", mode);
  }
}

applyTheme(localStorage.getItem("theme") || "system");
themeToggles.forEach((toggle) =>
  toggle.addEventListener("click", () => {
    const next = modes[(modes.indexOf(toggle.dataset.mode) + 1) % modes.length];
    applyTheme(next);
  }),
);

const revealObserver = new IntersectionObserver(
  (entries) => entries.forEach((entry) => entry.isIntersecting && (entry.target.dataset.visible = "true")),
  { threshold: 0.16 },
);
document.querySelectorAll("[data-reveal]").forEach((element) => revealObserver.observe(element));

const navLinks = [...document.querySelectorAll("[data-nav-link]")];
const indicator = document.querySelector("[data-nav-indicator]");
const sectionObserver = new IntersectionObserver(
  (entries) => {
    const current = entries.find((entry) => entry.isIntersecting);
    if (!current) return;
    const index = navLinks.findIndex((link) => link.dataset.section === current.target.id);
    navLinks.forEach((link, linkIndex) => (link.dataset.active = linkIndex === index));
    indicator.style.transform = `translateY(${index * 40}px)`;
  },
  { rootMargin: "-35% 0px -55%", threshold: 0 },
);
document.querySelectorAll("main > section[id]").forEach((section) => sectionObserver.observe(section));

function updateClock() {
  const time = new Intl.DateTimeFormat("en-GB", {
    timeZone: "Europe/Rome",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date());
  document.querySelectorAll("[data-time]").forEach((element) => (element.textContent = time));
}
updateClock();
setInterval(updateClock, 30_000);
document.querySelectorAll("[data-year]").forEach((element) => (element.textContent = new Date().getFullYear()));

document.querySelectorAll("[data-mobile-menu] a").forEach((link) =>
  link.addEventListener("click", () => link.closest("details").removeAttribute("open")),
);
