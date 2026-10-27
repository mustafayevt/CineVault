// Fills in the current version, installer size and release date from the public releases API, and
// tells Mac visitors that their version is still on its way. The buttons are written for Windows
// with the version-less releases/latest/download link, so they work without this script.
(function () {
  "use strict";
  var API = "https://api.github.com/repos/mustafayevt/CineVault-releases/releases/latest";
  var DOWNLOAD = "https://github.com/mustafayevt/CineVault-releases/releases/latest/download/";
  var RELEASES = "https://github.com/mustafayevt/CineVault-releases/releases";
  var SETUP = "CineVault-setup-x64.exe";

  function megabytes(bytes) {
    return (bytes / (1024 * 1024)).toFixed(1) + " MB";
  }

  function all(selector) {
    return Array.prototype.slice.call(document.querySelectorAll(selector));
  }

  function setAll(selector, text) {
    all(selector).forEach(function (node) {
      node.textContent = text;
    });
  }

  function isMac() {
    var platform =
      (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    if (/mac/i.test(platform)) return true;
    // iPads report a Mac user agent; they cannot run the app, so they keep the default page.
    return /Macintosh|Mac OS X/i.test(navigator.userAgent) && navigator.maxTouchPoints < 2;
  }

  // A Mac visitor: an honest "coming soon" with the Windows installer one click away.
  function showMac() {
    all("[data-dl]").forEach(function (button) {
      var label = button.querySelector("[data-dl-label]");
      var note = button.querySelector("[data-dl-note]");
      button.href = RELEASES;
      if (label) label.textContent = "macOS version coming soon";
      if (note) note.textContent = "See the releases page for news";
    });
    setAll("[data-dl-fine]", "CineVault runs on Windows 10 and 11 today. The macOS version is on its way.");
    all(".cta").forEach(function (cta) {
      var quiet = cta.querySelector(".btn-quiet");
      if (!quiet) return;
      quiet.href = DOWNLOAD + SETUP;
      quiet.textContent = "Download for Windows";
    });
  }

  if (isMac()) showMac();
  if (!window.fetch) return;
  fetch(API, { headers: { Accept: "application/vnd.github+json" } })
    .then(function (response) {
      if (!response.ok) throw new Error("HTTP " + response.status);
      return response.json();
    })
    .then(function (release) {
      var version = String(release.tag_name || "").replace(/^v/, "");
      if (/^\d+\.\d+\.\d+$/.test(version)) setAll("[data-version]", version);
      (release.assets || []).forEach(function (asset) {
        if (asset.name === SETUP && asset.size > 0) setAll("[data-size]", megabytes(asset.size));
      });
      var released = document.getElementById("released");
      if (released && release.published_at) {
        var date = new Date(release.published_at);
        if (!isNaN(date.getTime())) {
          released.textContent =
            "Version " + version + " released " +
            date.toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" });
        }
      }
    })
    .catch(function () {
      // Keep the version written into the page.
    });
})();

// Screenshots and cards rise in as they scroll into view. Reduced motion, or a browser without
// IntersectionObserver, shows everything at once.
(function () {
  "use strict";
  var items = Array.prototype.slice.call(document.querySelectorAll(".reveal"));
  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (still || !("IntersectionObserver" in window)) {
    items.forEach(function (item) {
      item.classList.add("is-in");
    });
    return;
  }
  var observer = new IntersectionObserver(
    function (entries) {
      entries.forEach(function (entry) {
        if (!entry.isIntersecting) return;
        entry.target.classList.add("is-in");
        observer.unobserve(entry.target);
      });
    },
    { rootMargin: "0px 0px -8% 0px", threshold: 0.08 }
  );
  items.forEach(function (item) {
    observer.observe(item);
  });
})();

// The rename strip cycles through the naming styles until someone picks one; reduced motion keeps
// it still on Clean, and the buttons always work.
(function () {
  "use strict";
  var STYLES = {
    clean: { out: "Dune - Part Two (2024)", template: "{Title} ({Year})" },
    detailed: {
      out: "Dune - Part Two (2024) [2160p] [Web-DL] [Dolby Vision, HDR10] [TrueHD Atmos 7.1]",
      template: "{Title} ({Year}) [{Resolution}] [{Source}] [{HDR}] [{Audio}]",
    },
    server: { out: "Dune - Part Two (2024) {tmdb-693134}", template: "{Title} ({Year}) {tmdb-{TMDB id}}" },
    custom: {
      out: "D/Dune - Part Two (2024) - 2160p",
      template: "{First letter}/{Title} ({Year}) - {Resolution}",
    },
  };
  var ORDER = ["clean", "detailed", "server", "custom"];
  var strip = document.querySelector(".strip");
  var out = document.querySelector("[data-style-out]");
  var template = document.querySelector("[data-style-template]");
  var buttons = Array.prototype.slice.call(document.querySelectorAll(".style-pick"));
  if (!strip || !out || !template || buttons.length === 0) return;

  var current = "clean";
  var timer = null;
  var touched = false;

  function show(name) {
    var style = STYLES[name];
    if (!style || name === current) return;
    current = name;
    buttons.forEach(function (button) {
      button.setAttribute("aria-pressed", String(button.getAttribute("data-style") === name));
    });
    out.classList.add("is-swapping");
    window.setTimeout(function () {
      out.textContent = style.out;
      template.textContent = style.template;
      out.classList.remove("is-swapping");
    }, 180);
  }

  function stop() {
    touched = true;
    if (timer !== null) window.clearInterval(timer);
    timer = null;
  }

  buttons.forEach(function (button) {
    button.addEventListener("click", function () {
      stop();
      show(button.getAttribute("data-style"));
    });
  });

  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (still || !("IntersectionObserver" in window)) return;
  // Start cycling only once the strip is on screen.
  var seen = new IntersectionObserver(function (entries) {
    if (!entries[0].isIntersecting || touched || timer !== null) return;
    timer = window.setInterval(function () {
      show(ORDER[(ORDER.indexOf(current) + 1) % ORDER.length]);
    }, 3000);
    seen.disconnect();
  });
  seen.observe(strip);
  strip.addEventListener("mouseenter", stop);
  strip.addEventListener("focusin", stop);
})();
