/**
 * The Membrane landing — gate visual + scroll reveals.
 * Canvas draws a full-bleed fail-closed control plane: authorization chain nodes
 * that pulse allow / block states. No live demo controls.
 */
(function () {
  "use strict";

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------- Scroll reveals ---------- */
  const reveals = document.querySelectorAll(".reveal");
  if (reveals.length && "IntersectionObserver" in window) {
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
            io.unobserve(entry.target);
          }
        }
      },
      { rootMargin: "0px 0px -8% 0px", threshold: 0.12 }
    );
    reveals.forEach((el) => io.observe(el));
  } else {
    reveals.forEach((el) => el.classList.add("is-visible"));
  }

  /* ---------- Demo chain step stagger ---------- */
  const steps = document.querySelectorAll(".demo-chain .step");
  if (steps.length && "IntersectionObserver" in window && !reduceMotion) {
    const stepIo = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          const list = entry.target.querySelectorAll(".step");
          list.forEach((step, i) => {
            step.style.transitionDelay = `${i * 70}ms`;
            step.classList.add("is-visible");
          });
          stepIo.unobserve(entry.target);
        }
      },
      { threshold: 0.2 }
    );
    const chain = document.querySelector(".demo-chain");
    if (chain) {
      steps.forEach((s) => {
        s.style.opacity = "0";
        s.style.transform = "translateX(-10px)";
        s.style.transition = "opacity 0.55s cubic-bezier(0.22,1,0.36,1), transform 0.55s cubic-bezier(0.22,1,0.36,1)";
      });
      const style = document.createElement("style");
      style.textContent = `.demo-chain .step.is-visible { opacity: 1 !important; transform: none !important; }`;
      document.head.appendChild(style);
      stepIo.observe(chain);
    }
  }

  /* ---------- Gate canvas visual ---------- */
  const canvas = document.querySelector(".gate-canvas");
  if (!canvas || !(canvas instanceof HTMLCanvasElement)) return;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  let w = 0;
  let h = 0;
  let dpr = 1;
  let raf = 0;
  let t0 = performance.now();

  const NODES = 7;
  const nodes = Array.from({ length: NODES }, (_, i) => ({
    i,
    // phase offset so nodes light in sequence
    phase: i * 0.85,
    // 0 allow, 1 block — middle nodes sometimes block
    kind: i === 3 || i === 5 ? "block" : "allow",
  }));

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    w = canvas.clientWidth;
    h = canvas.clientHeight;
    canvas.width = Math.floor(w * dpr);
    canvas.height = Math.floor(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  function nodePos(i, time) {
    // Vertical chain on the right half of the hero — dominant visual plane
    const padX = Math.min(w * 0.12, 120);
    const baseX = w * 0.62;
    const spread = Math.min(w * 0.28, 280);
    const y0 = h * 0.18;
    const y1 = h * 0.82;
    const t = NODES <= 1 ? 0 : i / (NODES - 1);
    const y = y0 + (y1 - y0) * t;
    // subtle lateral drift — purposeful, not decorative bounce
    const drift = Math.sin(time * 0.00045 + i * 1.1) * (spread * 0.04);
    const x = baseX + Math.sin(t * Math.PI) * (spread * 0.15) + drift;
    return { x: Math.max(padX, Math.min(w - padX, x)), y };
  }

  function draw(now) {
    const time = now - t0;
    ctx.clearRect(0, 0, w, h);

    // Atmospheric grid — sparse, structural
    ctx.save();
    ctx.strokeStyle = "rgba(158, 196, 210, 0.06)";
    ctx.lineWidth = 1;
    const grid = 56;
    const ox = (time * 0.008) % grid;
    for (let x = -grid + ox; x < w + grid; x += grid) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
      ctx.stroke();
    }
    for (let y = 0; y < h; y += grid) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }
    ctx.restore();

    // Chain links
    const positions = nodes.map((n) => nodePos(n.i, time));

    for (let i = 0; i < positions.length - 1; i++) {
      const a = positions[i];
      const b = positions[i + 1];
      const pulse = 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(time * 0.0018 + nodes[i].phase));
      const blocked = nodes[i + 1].kind === "block";
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.strokeStyle = blocked
        ? `rgba(163, 32, 32, ${0.25 + pulse * 0.35})`
        : `rgba(26, 122, 76, ${0.2 + pulse * 0.4})`;
      ctx.lineWidth = 2;
      ctx.stroke();

      // traveling packet along the link
      if (!reduceMotion) {
        const prog = (Math.sin(time * 0.0012 + nodes[i].phase) * 0.5 + 0.5);
        const px = a.x + (b.x - a.x) * prog;
        const py = a.y + (b.y - a.y) * prog;
        ctx.beginPath();
        ctx.arc(px, py, blocked ? 2.5 : 3, 0, Math.PI * 2);
        ctx.fillStyle = blocked ? "rgba(200, 80, 70, 0.85)" : "rgba(120, 210, 170, 0.9)";
        ctx.fill();
      }
    }

    // Nodes
    for (let i = 0; i < nodes.length; i++) {
      const n = nodes[i];
      const p = positions[i];
      const breath = 0.55 + 0.45 * Math.sin(time * 0.002 + n.phase);
      const isBlock = n.kind === "block";
      const color = isBlock ? [163, 32, 32] : [26, 122, 76];
      const r = isBlock ? 7 : 9;

      // outer ring
      ctx.beginPath();
      ctx.arc(p.x, p.y, r + 6 + breath * 3, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(${color[0]}, ${color[1]}, ${color[2]}, ${0.15 + breath * 0.2})`;
      ctx.lineWidth = 1.5;
      ctx.stroke();

      // core
      ctx.beginPath();
      ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(${color[0]}, ${color[1]}, ${color[2]}, ${0.55 + breath * 0.35})`;
      ctx.fill();
      ctx.strokeStyle = `rgba(232, 240, 244, ${0.35 + breath * 0.25})`;
      ctx.lineWidth = 1.25;
      ctx.stroke();

      // label tick — policy / model / tool feel without text clutter
      if (i === 0 || i === nodes.length - 1) {
        ctx.fillStyle = "rgba(196, 214, 222, 0.45)";
        ctx.font = "500 10px 'IBM Plex Mono', monospace";
        ctx.textAlign = "left";
        ctx.fillText(i === 0 ? "POLICY" : "EVIDENCE", p.x + r + 10, p.y + 3);
      }
    }

    // Gate plane — vertical membrane line that "holds" the chain
    const gx = w * 0.48;
    ctx.save();
    const shimmer = 0.12 + 0.08 * Math.sin(time * 0.001);
    const grad = ctx.createLinearGradient(gx, 0, gx, h);
    grad.addColorStop(0, "rgba(158, 212, 200, 0)");
    grad.addColorStop(0.35, `rgba(158, 212, 200, ${shimmer})`);
    grad.addColorStop(0.65, `rgba(158, 212, 200, ${shimmer})`);
    grad.addColorStop(1, "rgba(158, 212, 200, 0)");
    ctx.strokeStyle = grad;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(gx, h * 0.12);
    ctx.lineTo(gx, h * 0.88);
    ctx.stroke();
    ctx.restore();

    if (!reduceMotion) {
      raf = requestAnimationFrame(draw);
    }
  }

  function start() {
    resize();
    cancelAnimationFrame(raf);
    t0 = performance.now();
    if (reduceMotion) {
      draw(t0);
    } else {
      raf = requestAnimationFrame(draw);
    }
  }

  window.addEventListener("resize", () => {
    resize();
    if (reduceMotion) draw(performance.now());
  });

  // Pause when tab hidden
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      cancelAnimationFrame(raf);
    } else {
      start();
    }
  });

  start();
})();
