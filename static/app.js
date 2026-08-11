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

let revealObserver;
let sectionObserver;

function activateNavigation(section) {
  const links = [...document.querySelectorAll("[data-nav-link]")];
  const index = links.findIndex((link) => link.dataset.section === section);
  if (index < 0) return;
  links.forEach((link, linkIndex) => (link.dataset.active = linkIndex === index));
  document.querySelector("[data-nav-indicator]").style.transform = `translateY(${index * 40}px)`;
}

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

function initMain(focus = false) {
  revealObserver?.disconnect();
  sectionObserver?.disconnect();

  revealObserver = new IntersectionObserver(
    (entries) => entries.forEach((entry) => entry.isIntersecting && (entry.target.dataset.visible = "true")),
    { threshold: 0.16 },
  );
  document.querySelectorAll("#main [data-reveal]").forEach((element) => revealObserver.observe(element));
  document.querySelectorAll("[data-year]").forEach((element) => (element.textContent = new Date().getFullYear()));
  updateClock();

  if (location.pathname.startsWith("/quotes")) {
    activateNavigation("quotes");
  } else {
    const section = location.hash.slice(1) || "home";
    if (!location.hash) {
      document.documentElement.scrollTop = 0;
      document.body.scrollTop = 0;
    }
    activateNavigation(section);
    sectionObserver = new IntersectionObserver(
      (entries) => {
        const current = entries.find((entry) => entry.isIntersecting);
        if (current) activateNavigation(current.target.id);
      },
      { rootMargin: "-35% 0px -55%", threshold: 0 },
    );
    document.querySelectorAll("#main > section[id]").forEach((section) => sectionObserver.observe(section));
    if (location.hash) {
      requestAnimationFrame(() => {
        document.querySelector(location.hash)?.scrollIntoView();
        activateNavigation(section);
      });
    }
  }

  if (focus) document.querySelector("#main")?.focus({ preventScroll: true });
}

initMain();
document.addEventListener("htmx:afterSettle", (event) => {
  if (event.detail.target?.id === "main") initMain(true);
});
document.addEventListener("htmx:historyRestore", () => setTimeout(() => initMain(), 50));
window.addEventListener("popstate", () => setTimeout(() => initMain(), 50));

document.querySelectorAll("[data-mobile-menu] a").forEach((link) =>
  link.addEventListener("click", () => link.closest("details").removeAttribute("open")),
);

document.querySelectorAll("[popovertarget]").forEach((button) =>
  button.addEventListener("click", () => {
    const popover = document.getElementById(button.getAttribute("popovertarget"));
    const rect = button.getBoundingClientRect();
    popover.style.top = `${rect.bottom + 4}px`;
    popover.style.left = `${Math.max(16, Math.min(rect.left, innerWidth - 192))}px`;
  }),
);
