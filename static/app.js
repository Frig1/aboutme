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
  document.querySelector("[data-shell]")?.toggleAttribute("data-wide", location.pathname.startsWith("/admin/"));

  revealObserver = new IntersectionObserver(
    (entries) => entries.forEach((entry) => entry.isIntersecting && (entry.target.dataset.visible = "true")),
    { threshold: 0.16 },
  );
  document.querySelectorAll("#main [data-reveal]").forEach((element) => revealObserver.observe(element));
  document.querySelectorAll("[data-year]").forEach((element) => (element.textContent = new Date().getFullYear()));
  updateClock();

  if (location.pathname.startsWith("/admin/")) {
    activateNavigation("admin");
  } else if (location.pathname.startsWith("/quotes")) {
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

  initQuoteDashboard();
  initQuoteForm();

  if (focus) document.querySelector("#main")?.focus({ preventScroll: true });
}

function money(value) {
  return `€ ${value.toLocaleString("it-IT", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

function hourly(price, minimum, maximum) {
  if (!Number.isFinite(price) || !Number.isFinite(minimum) || minimum <= 0) return "—";
  const upper = price / minimum;
  if (!Number.isFinite(maximum) || maximum <= 0 || maximum === minimum) return `${money(upper)}/h`;
  if (maximum < minimum) return "—";
  return `${money(price / maximum)}–${money(upper)}/h`;
}

function initQuoteDashboard() {
  document.querySelectorAll("[data-rate-total]").forEach((row) => {
    const maximum = row.dataset.max ? Number(row.dataset.max) : NaN;
    row.querySelector("[data-rate-output]").textContent = hourly(
      Number(row.dataset.price),
      Number(row.dataset.min),
      maximum,
    );
  });
}

function initQuoteForm() {
  const form = document.querySelector("[data-quote-form]");
  if (!form || form.dataset.ready) return;
  form.dataset.ready = "true";
  const sections = form.querySelector("[data-sections]");
  const template = document.querySelector("[data-section-template]");

  function update() {
    const rows = [...sections.querySelectorAll("[data-quote-section]")];
    let totalPrice = 0;
    let totalMinimum = 0;
    let totalMaximum = 0;
    let hasEveryMaximum = true;

    rows.forEach((row, index) => {
      row.querySelector("[data-section-number]").textContent = index + 1;
      row.querySelectorAll("[name]").forEach((input) => {
        input.name = input.name.replace(/sections\.(?:\d+|__INDEX__)/, `sections.${index}`);
      });
      row.querySelector("[data-move-up]").disabled = index === 0;
      row.querySelector("[data-move-down]").disabled = index === rows.length - 1;
      row.querySelector("[data-remove-section]").disabled = rows.length === 1;

      const price = Number(row.querySelector("[data-price-euros]").value);
      const minimum = Number(row.querySelector("[data-min-hours]").value);
      const maximumInput = row.querySelector("[data-max-hours]").value;
      const maximum = maximumInput === "" ? NaN : Number(maximumInput);
      row.querySelector("[data-section-rate]").textContent = hourly(price, minimum, maximum);
      if (Number.isFinite(price)) totalPrice += price;
      if (Number.isFinite(minimum)) totalMinimum += minimum;
      if (Number.isFinite(maximum)) totalMaximum += maximum;
      else hasEveryMaximum = false;
    });

    form.querySelector("[data-total-price]").textContent = money(totalPrice);
    form.querySelector("[data-total-rate]").textContent = hourly(
      totalPrice,
      totalMinimum,
      hasEveryMaximum ? totalMaximum : NaN,
    );
  }

  form.addEventListener("input", update);
  form.addEventListener("submit", update);
  form.addEventListener("click", (event) => {
    const button = event.target.closest("button");
    if (!button) return;
    const row = button.closest("[data-quote-section]");
    if (button.matches("[data-add-section]")) {
      sections.append(template.content.cloneNode(true));
    } else if (button.matches("[data-remove-section]") && sections.children.length > 1) {
      row.remove();
    } else if (button.matches("[data-move-up]") && row.previousElementSibling) {
      sections.insertBefore(row, row.previousElementSibling);
    } else if (button.matches("[data-move-down]") && row.nextElementSibling) {
      sections.insertBefore(row.nextElementSibling, row);
    } else {
      return;
    }
    update();
  });
  update();
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
