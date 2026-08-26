(() => {
  "use strict";

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------------------------------------------------------------------
   * Stardate: TNG-era approximation, purely decorative
   * ------------------------------------------------------------------- */
  function computeStardate() {
    const now = new Date();
    const start = new Date(now.getFullYear(), 0, 1);
    const dayOfYear = Math.floor((now - start) / 86400000);
    const base = (now.getFullYear() - 2323) * 1000; // arbitrary offset, "TNG" era
    const fraction = (dayOfYear / 365) * 1000;
    const stardate = Math.abs(base) + fraction + now.getHours() / 24;
    return stardate.toFixed(1);
  }

  const stardateEl = document.getElementById("stardate-value");
  if (stardateEl) stardateEl.textContent = computeStardate();

  /* ---------------------------------------------------------------------
   * Collapsible navigation (below the breakpoint)
   *
   * Above it, the sidebar shows everything and the button is hidden in CSS: this code then
   * never runs, `aria-expanded` staying at `false` with no visible effect.
   * ------------------------------------------------------------------- */
  const sidebar = document.querySelector(".sidebar");
  const navToggle = document.getElementById("nav-toggle");

  if (sidebar && navToggle) {
    const setNavOpen = (open) => {
      sidebar.classList.toggle("nav-open", open);
      navToggle.setAttribute("aria-expanded", String(open));
      navToggle.setAttribute(
        "aria-label",
        open ? "Close navigation" : "Open navigation"
      );
    };

    navToggle.addEventListener("click", () => {
      setNavOpen(!sidebar.classList.contains("nav-open"));
    });

    // Close again after picking a destination: on mobile the menu covers the content, so
    // leaving it open would hide the section just reached.
    sidebar.querySelectorAll(".pillnav a").forEach((link) => {
      link.addEventListener("click", () => setNavOpen(false));
    });

    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && sidebar.classList.contains("nav-open")) {
        setNavOpen(false);
        navToggle.focus();
      }
    });

    document.addEventListener("click", (e) => {
      if (!sidebar.classList.contains("nav-open")) return;
      if (!sidebar.contains(e.target)) setNavOpen(false);
    });
  }

  /* ---------------------------------------------------------------------
   * Scrollspy: highlights the active tab in the sidebar
   * ------------------------------------------------------------------- */
  const sections = document.querySelectorAll("main > section[id]");
  const pills = document.querySelectorAll(".pillnav .pill");

  if (sections.length && pills.length && "IntersectionObserver" in window) {
    const byId = new Map();
    pills.forEach((p) => byId.set(p.getAttribute("href").slice(1), p));

    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          const pill = byId.get(entry.target.id);
          if (!pill) return;
          if (entry.isIntersecting) {
            pills.forEach((p) => p.setAttribute("aria-current", "false"));
            pill.setAttribute("aria-current", "true");
          }
        });
      },
      { rootMargin: "-40% 0px -50% 0px", threshold: 0 }
    );

    sections.forEach((s) => observer.observe(s));
  }

  /* ---------------------------------------------------------------------
   * Hero console: human / agent exchange simulation, fixed text
   * (not a real session: purely decorative, never presented as
   * an actual capture). Cut short if prefers-reduced-motion.
   * ------------------------------------------------------------------- */
  const consoleBody = document.getElementById("console-typer");
  if (!consoleBody) return;

  const transcript = [
    { who: "human", text: "PS C:\\Users\\pilot> " },
    { who: "agent", text: "beammeup send my-work \"npm run build\\r\"" },
    { who: "system", text: "\n> Building...\n> Build completed in 4.2s\n\n" },
    { who: "human", text: "PS C:\\Users\\pilot> " },
    { who: "agent", text: "git status" },
    { who: "system", text: "\nnothing to commit, working tree clean\n\n" },
    { who: "human", text: "PS C:\\Users\\pilot> _" },
  ];

  function renderStatic() {
    consoleBody.textContent = transcript.map((l) => l.text).join("");
  }

  if (reduceMotion) {
    renderStatic();
    return;
  }

  let lineIndex = 0;
  let charIndex = 0;
  let buffer = "";

  function typeNext() {
    if (lineIndex >= transcript.length) {
      consoleBody.innerHTML = buffer.replace(/_$/, '<span class="caret"> </span>');
      return;
    }
    const current = transcript[lineIndex];
    if (charIndex < current.text.length) {
      buffer += current.text[charIndex];
      consoleBody.textContent = buffer;
      charIndex++;
      const speed = current.who === "system" ? 6 : 24;
      window.setTimeout(typeNext, speed);
    } else {
      lineIndex++;
      charIndex = 0;
      window.setTimeout(typeNext, current.who === "human" ? 350 : 120);
    }
  }

  // Only start the animation once the console is visible, so as not to
  // "consume" the sequence before the visitor has seen it.
  if ("IntersectionObserver" in window) {
    const heroObserver = new IntersectionObserver(
      (entries, obs) => {
        if (entries.some((e) => e.isIntersecting)) {
          typeNext();
          obs.disconnect();
        }
      },
      { threshold: 0.4 }
    );
    heroObserver.observe(consoleBody);
  } else {
    typeNext();
  }
})();
