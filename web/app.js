// poecraft web UI (vanilla JS, talks to the local Rust server).
"use strict";
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const api = async (path, body) => {
  const r = await fetch(path, body ? { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) } : {});
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
};
const fmt = (v, d = 0) => (v === null || v === undefined || !isFinite(v)) ? "-" : Number(v).toLocaleString(undefined, { maximumFractionDigits: d, minimumFractionDigits: d });
let DIVINE = 200;

// ------------------------------------------------------------------ tabs
document.querySelectorAll("#tabs button").forEach((b) => b.addEventListener("click", () => {
  document.querySelectorAll("#tabs button").forEach((x) => x.classList.toggle("active", x === b));
  document.querySelectorAll(".tab").forEach((t) => t.classList.toggle("active", t.id === b.dataset.tab));
}));

// ------------------------------------------------------------------ builder state
const state = { base: "Astral Plate", ilvl: 86, rarity: "rare", influences: ["hunter", "warlord"], mods: [] };
let lastPool = null;

function itemDto() {
  return { base: state.base, ilvl: state.ilvl, rarity: state.rarity, influences: state.influences, mods: state.mods };
}

async function refresh() {
  try {
    const r = await api("/api/pool", { item: itemDto() });
    lastPool = r;
    renderItem(r);
    renderPool();
    renderHints(r.hints);
    await renderText();
  } catch (e) {
    $("#itemCard").innerHTML = `<div class="err">${esc(e.message)}</div>`;
  }
}

function infTag(i) { return i ? `<span class="inf ${i}">${i[0].toUpperCase() + i.slice(1)}</span>` : ""; }
function tierTag(m) {
  if (m.crafted) return `<span class="tier">craft</span>`;
  if (m.tier === 0) return `<span class="tier t0">ELEV</span>`;
  return `<span class="tier ${m.tier === 1 ? "t1" : ""}">T${m.tier}</span>`;
}

function renderItem(r) {
  const it = r.item;
  const mods = it.mods.map((m, i) => `<div class="mod"><span class="side-${m.side}">${m.side[0].toUpperCase()}</span>${tierTag(m)}${infTag(m.influence)}
      <span class="${m.crafted ? "crafted" : m.tier === 0 ? "elev" : ""}">${esc(m.text)}${m.fractured ? " <span class='small muted'>(fractured)</span>" : ""}</span>
      <button class="x" data-rm="${esc(m.id)}" title="remove">×</button></div>`).join("");
  const probs = r.problems.length ? `<div class="problems">${r.problems.map(esc).join("<br>")}</div>` : `<div class="ok">Legal item</div>`;
  $("#itemCard").innerHTML = `<h3>${esc(it.rarity === "rare" ? "Rare" : it.rarity === "magic" ? "Magic" : "Normal")} ${esc(it.base)}</h3>
    <div class="base">${esc(it.item_class)} · item level ${it.ilvl} · ${it.influences.map(infTag).join(" ") || "no influence"}</div>
    ${it.implicits.map((t) => `<div class="small muted">${esc(t)} (implicit)</div>`).join("")}
    <div style="margin-top:8px">${mods || '<span class="muted small">No mods yet: add them from the mod pool.</span>'}</div>
    <div class="small muted" style="margin-top:6px">open prefixes ${it.open_prefix} · open suffixes ${it.open_suffix}</div>${probs}`;
  document.querySelectorAll("[data-rm]").forEach((b) => b.addEventListener("click", () => {
    state.mods = state.mods.filter((m) => m.id !== b.dataset.rm);
    refresh();
  }));
}

function addMod(id, crafted = false) {
  state.mods.push({ id, crafted, fractured: false });
  refresh();
}

function renderPool() {
  if (!lastPool) return;
  const f = $("#poolFilter").value.toLowerCase();
  const side = $("#poolSide").value;
  const showBlocked = $("#showBlocked").checked;
  const rows = lastPool.pool
    .filter((e) => (!side || e.side === side) && (showBlocked || !e.blocked))
    .filter((e) => !f || e.text.toLowerCase().includes(f) || e.tags.some((t) => t.includes(f)) || (e.influence || "").includes(f))
    .sort((a, b) => (a.side > b.side ? 1 : -1) || (b.chance - a.chance))
    .slice(0, 400);
  $("#poolBody").innerHTML = rows.map((e) => `<tr class="${e.blocked ? "blocked" : ""}">
      <td><button data-add="${esc(e.id)}" ${e.blocked ? `disabled title="blocked: ${e.blocked}"` : ""}>+</button></td>
      <td class="side-${e.side}">${e.side}</td><td>${tierTag(e)}</td>
      <td>${infTag(e.influence)} ${esc(e.text)} <span class="small muted">${esc(e.tags.join(", "))}</span></td>
      <td>${e.level}</td><td>${e.weight}</td><td>${e.blocked ? esc(e.blocked) : fmt(e.chance * 100, 2) + "%"}</td></tr>`).join("");
  document.querySelectorAll("[data-add]").forEach((b) => b.addEventListener("click", () => addMod(b.dataset.add)));
  $("#elevBody").innerHTML = lastPool.elevated.map((e) => `<tr><td><button data-addel="${esc(e.id)}">+</button></td><td class="side-${e.side}">${e.side}</td><td>${infTag(e.influence)} ${esc(e.text)}</td></tr>`).join("") || `<tr><td class="muted small">No influenced families with elevated versions here.</td></tr>`;
  document.querySelectorAll("[data-addel]").forEach((b) => b.addEventListener("click", () => addMod(b.dataset.addel)));
  const bf = $("#benchFilter").value.toLowerCase();
  $("#benchBody").innerHTML = lastPool.bench.filter((b) => !bf || b.text.toLowerCase().includes(bf)).slice(0, 200).map((b) => `<tr><td><button data-addcr="${esc(b.id)}">+</button></td><td class="side-${b.side}">${b.side}</td><td>${esc(b.text)}</td><td class="small muted">${esc(Object.entries(b.cost).map(([k, v]) => v + " " + k).join(", "))}</td></tr>`).join("");
  document.querySelectorAll("[data-addcr]").forEach((b) => b.addEventListener("click", () => addMod(b.dataset.addcr, true)));
}

function renderHints(h) {
  $("#hints").innerHTML = h.length ? h.map((x) => `<li>${esc(x.text)}</li>`).join("") : `<li class="muted">No special tag tricks for this base/influence.</li>`;
}

async function renderText() {
  try {
    const r = await api("/api/item/text", { item: itemDto(), roll: Number($("#roll").value), quality: Number($("#quality").value) });
    $("#pobText").textContent = r.text;
  } catch (e) {
    $("#pobText").textContent = e.message;
  }
}

$("#copyBtn").addEventListener("click", async () => {
  await renderText();
  try {
    await navigator.clipboard.writeText($("#pobText").textContent);
    $("#copyBtn").textContent = "Copied!";
  } catch { $("#copyBtn").textContent = "Copy failed - select the text"; }
  setTimeout(() => ($("#copyBtn").textContent = "Copy item"), 1500);
});
["#roll", "#quality"].forEach((s) => $(s).addEventListener("input", renderText));
["#poolFilter", "#poolSide", "#showBlocked", "#benchFilter"].forEach((s) => $(s).addEventListener("input", renderPool));

$("#base").addEventListener("input", async () => {
  const q = $("#base").value;
  const list = await api("/api/bases?q=" + encodeURIComponent(q));
  $("#baselist").innerHTML = list.map((b) => `<option value="${esc(b.name)}">${esc(b.class)}</option>`).join("");
  if (list.some((b) => b.name.toLowerCase() === q.toLowerCase())) {
    state.base = q; state.mods = []; refresh();
  }
});
$("#ilvl").addEventListener("change", () => { state.ilvl = Number($("#ilvl").value); refresh(); });
$("#rarity").addEventListener("change", () => { state.rarity = $("#rarity").value; refresh(); });

function renderInfluences(all) {
  $("#influences").innerHTML = all.map((i) => `<span class="chip ${state.influences.includes(i) ? "on" : ""}" data-inf="${i}">${i}</span>`).join("");
  document.querySelectorAll("[data-inf]").forEach((c) => c.addEventListener("click", () => {
    const i = c.dataset.inf;
    if (state.influences.includes(i)) state.influences = state.influences.filter((x) => x !== i);
    else if (state.influences.length < 2) state.influences.push(i);
    state.mods = state.mods.filter((m) => true);
    renderInfluences(all);
    refresh();
  }));
}

$("#importBtn").addEventListener("click", async () => {
  try {
    const r = await api("/api/item/parse", { text: $("#paste").value });
    Object.assign(state, { base: r.item.base, ilvl: r.item.ilvl, rarity: r.item.rarity, influences: r.item.influences, mods: r.item.mods });
    $("#base").value = state.base; $("#ilvl").value = state.ilvl; $("#rarity").value = state.rarity;
    renderInfluences(META.influences);
    const notes = [];
    if (r.unmatched.length) notes.push(`<div class="problems">Not recognised: ${r.unmatched.map(esc).join("; ")}</div>`);
    if (r.ambiguous.length) notes.push(`<div class="problems" style="color:var(--warn)">${r.ambiguous.map(esc).join("<br>")}</div>`);
    $("#importMsg").innerHTML = notes.join("") || `<div class="ok">Imported ${r.item.mods.length} mods.</div>`;
    refresh();
  } catch (e) { $("#importMsg").innerHTML = `<div class="err">${esc(e.message)}</div>`; }
});

// ------------------------------------------------------------------ jobs + progress
async function runJob(startPath, body, progEl, onDone) {
  const bar = progEl.querySelector(".bar"), fill = bar.firstElementChild, txt = progEl.querySelector(".ptext");
  progEl.hidden = false;
  let job;
  try { job = (await api(startPath, body)).job; } catch (e) { progEl.hidden = true; onDone(null, e.message); return; }
  let phaseStart = performance.now(), lastPhase = "";
  for (;;) {
    await new Promise((r) => setTimeout(r, 150));
    let s;
    try { s = await api("/api/jobs/" + job); } catch (e) { onDone(null, e.message); return; }
    if (s.phase !== lastPhase) { lastPhase = s.phase; phaseStart = performance.now(); }
    if (s.total > 0) {
      const f = s.done / s.total, el = (performance.now() - phaseStart) / 1000;
      bar.classList.remove("indeterminate");
      fill.style.width = (f * 100).toFixed(1) + "%";
      const eta = f > 0 ? el / f - el : 0;
      txt.textContent = `${s.label} · ${s.phase} · ${fmt(s.done)}/${fmt(s.total)} (${(f * 100).toFixed(1)}%) · ${s.elapsed.toFixed(1)}s · eta ${eta.toFixed(1)}s`;
    } else {
      bar.classList.add("indeterminate");
      txt.textContent = `${s.label} · ${s.phase || "starting"}… · ${s.elapsed.toFixed(1)}s`;
    }
    if (s.status !== "running") {
      progEl.hidden = true;
      onDone(s.status === "done" ? s.result : null, s.error);
      return;
    }
  }
}

function reportHtml(r) {
  const dv = r.divine || DIVINE;
  const stat = (l, v) => `<div class="stat"><div class="v">${v}</div><div class="l">${l}</div></div>`;
  const p = r.percentiles || {};
  let html = `<div class="card"><h2>${esc(r.name)} <span class="muted small">${esc(r.method)}${r.runs ? ", " + fmt(r.runs) + " runs" : ""}</span></h2><div class="stats">
    ${stat("success", fmt(r.p_success * 100, 1) + "%")}
    ${stat("expected cost", fmt(r.mean_chaos / dv, 2) + " div")}
    ${stat("cost per success", fmt(r.cost_per_success / dv, 2) + " div")}
    ${stat("std dev", fmt(r.std_chaos / dv, 2) + " div")}
    ${p["90"] !== undefined ? stat("p50 / p90", fmt(p["50"] / dv, 1) + " / " + fmt(p["90"] / dv, 1) + " div") : ""}
    ${r.p_lose_base !== null && r.p_lose_base !== undefined ? stat("lose a bought base", fmt(r.p_lose_base * 100, 1) + "%") : ""}
    ${r.revenue !== null && r.revenue !== undefined ? stat("expected profit", fmt((r.revenue - r.mean_chaos) / dv, 2) + " div") : ""}
  </div>`;
  const tracks = Object.entries(r.tracks || {});
  if (tracks.length) html += `<table class="list">${tracks.map(([k, v]) => `<tr><td>${esc(k)}</td><td style="text-align:right">${fmt(v * 100, 2)}%</td></tr>`).join("")}</table>`;
  const counts = Object.entries(r.mean_counts || {}).filter(([k]) => !k.startsWith("_")).sort((a, b) => b[1] - a[1]);
  html += `<details><summary class="small muted">expected currency used</summary><table class="list">${counts.map(([k, v]) => `<tr><td>${esc(k)}</td><td style="text-align:right">${fmt(v, 3)}</td></tr>`).join("")}</table></details>`;
  if (r.agreement) html += `<p class="small muted">exact vs Monte Carlo: ${esc(r.agreement)}</p>`;
  if (r.uncertain_rules && r.uncertain_rules.length) html += `<p class="small" style="color:var(--warn)">Depends on unverified/disputed rules: ${r.uncertain_rules.map((x) => esc(x.id) + (x.question ? " (" + x.question + ")" : "")).join(", ")}</p>`;
  if (r.missing_prices && r.missing_prices.length) html += `<p class="small problems">No price for: ${r.missing_prices.map(esc).join(", ")}</p>`;
  if (r.example_item) html += `<details><summary class="small muted">example finished item</summary><pre>${esc(r.example_item)}</pre></details>`;
  return html + `</div>`;
}

// ------------------------------------------------------------------ simulator
let RECIPES = [];
async function loadRecipes() {
  RECIPES = await api("/api/recipes");
  $("#recipeSel").innerHTML = RECIPES.map((r, i) => `<option value="${i}">${esc(r.path)}</option>`).join("");
  const pick = RECIPES.findIndex((r) => r.path.endsWith("endgame_chest_no_preview_safe.craft"));
  $("#recipeSel").value = String(pick >= 0 ? pick : 0);
  $("#recipeSrc").value = RECIPES[$("#recipeSel").value]?.source || "";
  $("#cmpList").innerHTML = RECIPES.map((r, i) => `<span class="chip" data-cmp="${i}">${esc(r.path.replace("recipes/", ""))}</span>`).join("");
  document.querySelectorAll("[data-cmp]").forEach((c) => c.addEventListener("click", () => c.classList.toggle("on")));
}
$("#recipeSel").addEventListener("change", () => { $("#recipeSrc").value = RECIPES[$("#recipeSel").value].source; });
$("#runBtn").addEventListener("click", () => {
  const r = RECIPES[$("#recipeSel").value];
  $("#runBtn").disabled = true;
  $("#simResult").innerHTML = "";
  runJob("/api/jobs/run", { source: $("#recipeSrc").value, path: r?.path, name: r?.name, runs: Number($("#runs").value), mode: $("#mode").value }, $("#simProgress"), (res, err) => {
    $("#runBtn").disabled = false;
    $("#simResult").innerHTML = err ? `<div class="card err">${esc(err)}</div>` : reportHtml(res);
  });
});

// ------------------------------------------------------------------ compare
$("#cmpBtn").addEventListener("click", () => {
  const sel = [...document.querySelectorAll("[data-cmp].on")].map((c) => RECIPES[c.dataset.cmp]);
  if (!sel.length) { $("#cmpResult").innerHTML = `<div class="card muted">Pick recipes to compare.</div>`; return; }
  $("#cmpBtn").disabled = true;
  $("#cmpResult").innerHTML = "";
  runJob("/api/jobs/compare", { recipes: sel.map((r) => ({ source: r.source, path: r.path, name: r.name })), runs: Number($("#cmpRuns").value) }, $("#cmpProgress"), (res, err) => {
    $("#cmpBtn").disabled = false;
    if (err) { $("#cmpResult").innerHTML = `<div class="card err">${esc(err)}</div>`; return; }
    const dv = res.reports[0]?.divine || DIVINE;
    const rows = res.reports.map((r, i) => `<tr><td>${i + 1}</td><td>${esc(r.name)}</td><td>${fmt(r.p_success * 100, 1)}%</td><td>${fmt(r.mean_chaos / dv, 2)}</td><td><b>${fmt(r.cost_per_success / dv, 2)}</b></td><td>${fmt(r.std_chaos / dv, 2)}</td><td>${r.percentiles["90"] !== undefined ? fmt(r.percentiles["90"] / dv, 1) : "-"}</td><td>${r.p_lose_base !== null ? fmt(r.p_lose_base * 100, 1) + "%" : "-"}</td></tr>`).join("");
    $("#cmpResult").innerHTML = `<div class="card"><h2>Ranking (cost per success, divines)</h2><table class="pool"><thead><tr><th>#</th><th>recipe</th><th>success</th><th>E[cost]</th><th>per success</th><th>std</th><th>p90</th><th>lose base</th></tr></thead><tbody>${rows}</tbody></table></div>` + res.reports.map(reportHtml).join("");
  });
});

// ------------------------------------------------------------------ rules / prices
async function loadRules() {
  const rules = await api("/api/rules");
  $("#rulesBody").innerHTML = rules.map((r) => `<tr><td><code>${esc(r.id)}</code></td><td class="status-${r.status}">${esc(r.status.replace("_", " "))}${r.question ? " · " + r.question : ""}</td><td>${esc(r.summary)}<div class="small muted">${esc(r.source)}</div></td></tr>`).join("");
}
let PRICES = {};
async function loadPrices() {
  const p = await api("/api/prices");
  PRICES = p.prices;
  $("#priceMeta").textContent = `${p.meta.league || ""} · fetched ${p.meta.fetched || "?"} · ${p.meta.source || ""}`;
  renderPrices();
}
function renderPrices() {
  const f = $("#priceFilter").value.toLowerCase();
  $("#priceBody").innerHTML = Object.entries(PRICES).filter(([k]) => !f || k.toLowerCase().includes(f)).map(([k, v]) => `<tr><td>${esc(k)}</td><td style="text-align:right">${fmt(v, 3)}c</td></tr>`).join("");
}
$("#priceFilter").addEventListener("input", renderPrices);
$("#fetchPrices").addEventListener("click", async () => {
  $("#fetchPrices").disabled = true;
  $("#fetchMsg").textContent = "fetching from poe.ninja…";
  try {
    const r = await api("/api/prices/fetch", {});
    $("#fetchMsg").textContent = r.log.trim().split("\n").slice(-3).join(" · ");
    await loadPrices();
    META = await api("/api/meta"); DIVINE = META.divine || DIVINE;
  } catch (e) { $("#fetchMsg").textContent = e.message; }
  $("#fetchPrices").disabled = false;
});

// ------------------------------------------------------------------ boot
let META = { influences: [] };
(async () => {
  META = await api("/api/meta");
  DIVINE = META.divine || DIVINE;
  $("#meta").textContent = `game data ${META.source.game_version} · ${META.prices.league || ""} prices ${META.prices.fetched || ""} · divine ${fmt(DIVINE)}c`;
  renderInfluences(META.influences);
  await refresh();
  await loadRecipes();
  await loadRules();
  await loadPrices();
})();
