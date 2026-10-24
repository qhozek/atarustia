import { metal } from "./metal.js";

const TEXTS = {
  en: {
    auth_title: "Confirm it's you",
    auth_text: "Your bot will send a request to Telegram. Press «Confirm» in it to open the web interface.",
    auth_send: "Send request",
    auth_wait: "Waiting for confirmation in Telegram",
    auth_link: "Or send {command} to Saved Messages: a one-time link will come without the bot.",
    app_title: "Telegram app",
    app_text: "ataraxia signs in on behalf of an app. Take an official one or use your own pair.",
    official: "official",
    own_app: "My own app",
    own_app_meta: "API ID and hash from my.telegram.org",
    api_id: "API ID",
    api_hash: "API hash",
    api_where: "Create an app at",
    next: "Continue",
    setup_title: "Sign in to Telegram",
    add_title: "Add an account",
    by_qr: "QR code",
    by_phone: "Phone number",
    qr_1: "Open Telegram on your phone",
    qr_2: "Settings → Devices → Link Desktop Device",
    qr_3: "Point the camera at this code",
    phone: "Phone number",
    get_code: "Get code",
    change_app: "Change Telegram app",
    code_title: "Enter the code",
    code_via: { app: "Sent to Telegram on your other device for {phone}", sms: "Sent by SMS to {phone}", call: "You'll get a call to {phone}", fragmentsms: "Sent to {phone} via Fragment", emailcode: "Sent to the email of {phone}" },
    code_other: "Sent to {phone}",
    sign_in: "Sign in",
    other_number: "Use another number",
    password_title: "Two-step verification",
    password_text: "The account is protected with a cloud password.",
    password: "Password",
    password_hint: "Hint: {hint}",
    start_over: "Start over",
    done_title: "Welcome, {name}",
    done_anon: "Account added",
    done_text: "ataraxia is restarting. In a few seconds your bot will write to you in Telegram.",
    limit_title: "One account only",
    limit_text: "This hosting allows one account per userbot.",
    errors: {
      network: "No connection to ataraxia",
      auth: "Confirm the login again",
      bad_request: "Bad request, reload the page",
      ratelimit: "Too many requests, try again in a few minutes",
      bot: "The bot couldn't send the request to Telegram",
      timeout: "Time is up, send the request again",
      link_invalid: "The link is used or expired, get a new one with {command}",
      session: "Couldn't save the session, see the logs",
      app_invalid: "Check the API ID and hash",
      no_app: "Choose a Telegram app first",
      limit: "This hosting allows one account",
      phone_invalid: "Invalid phone number",
      phone_unoccupied: "There's no Telegram account with this number",
      flood: "Telegram asks to wait {time}",
      captcha: "Telegram wants a captcha for this login, sign in with the QR code",
      payment: "Telegram wants a payment to send the code, sign in with the QR code",
      code_invalid: "Wrong code",
      code_expired: "The code has expired, request a new one",
      password_invalid: "Wrong password",
      no_login: "The login was reset, start over",
      telegram: "Telegram didn't answer, try again",
    },
    units: ["h", "min", "s"],
  },
  ru: {
    auth_title: "Подтверди, что это ты",
    auth_text: "Бот пришлёт запрос в Telegram. Нажми в нём «Подтвердить», и веб-интерфейс откроется.",
    auth_send: "Отправить запрос",
    auth_wait: "Жду подтверждения в Telegram",
    auth_link: "Или отправь {command} в Избранное: придёт одноразовая ссылка, бот не нужен.",
    app_title: "Приложение Telegram",
    app_text: "ataraxia входит от имени приложения. Возьми официальное или свою пару.",
    official: "официальное",
    own_app: "Своё приложение",
    own_app_meta: "API ID и hash с my.telegram.org",
    api_id: "API ID",
    api_hash: "API hash",
    api_where: "Создать приложение можно на",
    next: "Дальше",
    setup_title: "Вход в Telegram",
    add_title: "Новый аккаунт",
    by_qr: "QR-код",
    by_phone: "Номер телефона",
    qr_1: "Открой Telegram на телефоне",
    qr_2: "Настройки → Устройства → Подключить устройство",
    qr_3: "Наведи камеру на этот код",
    phone: "Номер телефона",
    get_code: "Получить код",
    change_app: "Сменить приложение Telegram",
    code_title: "Введи код",
    code_via: { app: "Отправили в Telegram на другом устройстве, номер {phone}", sms: "Отправили по SMS на {phone}", call: "Сейчас позвонят на {phone}", fragmentsms: "Отправили на {phone} через Fragment", emailcode: "Отправили на почту аккаунта {phone}" },
    code_other: "Отправили на {phone}",
    sign_in: "Войти",
    other_number: "Другой номер",
    password_title: "Облачный пароль",
    password_text: "Аккаунт защищён двухэтапной проверкой.",
    password: "Пароль",
    password_hint: "Подсказка: {hint}",
    start_over: "Начать заново",
    done_title: "Привет, {name}",
    done_anon: "Аккаунт добавлен",
    done_text: "ataraxia перезапускается. Через несколько секунд бот напишет тебе в Telegram.",
    limit_title: "Только один аккаунт",
    limit_text: "Этот хостинг разрешает один аккаунт на юзербота.",
    errors: {
      network: "Нет связи с ataraxia",
      auth: "Подтверди вход ещё раз",
      bad_request: "Неверный запрос, обнови страницу",
      ratelimit: "Слишком много запросов, попробуй через пару минут",
      bot: "Бот не смог отправить запрос в Telegram",
      timeout: "Время вышло, отправь запрос ещё раз",
      link_invalid: "Ссылка уже использована или устарела, получи новую командой {command}",
      session: "Не удалось сохранить сессию, смотри логи",
      app_invalid: "Проверь API ID и hash",
      no_app: "Сначала выбери приложение Telegram",
      limit: "Этот хостинг разрешает один аккаунт",
      phone_invalid: "Неверный номер",
      phone_unoccupied: "На этот номер нет аккаунта Telegram",
      flood: "Telegram просит подождать {time}",
      captcha: "Telegram требует капчу для этого входа, войди по QR-коду",
      payment: "Telegram хочет оплату за отправку кода, войди по QR-коду",
      code_invalid: "Неверный код",
      code_expired: "Код устарел, запроси новый",
      password_invalid: "Неверный пароль",
      no_login: "Вход сброшен, начни заново",
      telegram: "Telegram не ответил, попробуй ещё раз",
    },
    units: ["ч", "мин", "с"],
  },
};

const T = TEXTS[/^(ru|uk|be|kk)\b/i.test(navigator.language) ? "ru" : "en"];
document.documentElement.lang = T === TEXTS.ru ? "ru" : "en";

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
const fill = (text, values) => text.replace(/\{(\w+)\}/g, (_, key) => values[key] ?? "");
const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));
const mobile = matchMedia("(pointer: coarse)").matches;
const base = location.pathname.replace(/\/+$/, "") + "/";

class ApiError extends Error {}

async function api(path, body) {
  let response;
  try {
    response = await fetch(base + "api/" + path, body === undefined
      ? { credentials: "same-origin", cache: "no-store" }
      : { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  } catch {
    throw Object.assign(new ApiError("network"), { code: "network" });
  }
  const data = await response.json().catch(() => ({ error: "network" }));
  if (!response.ok) throw Object.assign(new ApiError(data.error), { code: data.error || "network", ...data });
  return data;
}

function duration(seconds) {
  const [h, m, s] = T.units;
  const parts = [[Math.floor(seconds / 3600), h], [Math.floor((seconds % 3600) / 60), m], [seconds % 60, s]];
  return parts.filter(([value]) => value).map(([value, unit]) => `${value} ${unit}`).join(" ") || `0 ${s}`;
}

function errorText(error) {
  const text = T.errors[error.code] || T.errors.telegram;
  return fill(text, { time: duration(error.seconds || 0), command: state?.command || ".weburl" });
}

let screen = null;

function show(name) {
  if (screen?.dataset.screen === name) return screen;
  screen?.classList.remove("on");
  screen = $(`[data-screen="${name}"]`);
  $$(".error", screen).forEach((e) => (e.textContent = ""));
  screen.classList.add("on");
  $("input:not([hidden])", screen)?.focus({ preventScroll: true });
  return screen;
}

function fail(error, where = screen) {
  if (error.code === "auth") return boot();
  if (error.code === "no_login") return route(null);
  const line = where.matches(".error") ? where : $(".error", where);
  line.textContent = errorText(error);
  line.classList.remove("shake");
  void line.offsetWidth;
  line.classList.add("shake");
}

async function busy(button, job) {
  if (button.disabled) return;
  button.disabled = true;
  button.classList.add("busy");
  try {
    return await job();
  } finally {
    button.disabled = false;
    button.classList.remove("busy");
  }
}

$$("[data-t]").forEach((element) => (element.textContent = T[element.dataset.t]));

const plate = $("#plate");
const surface = metal($("canvas", plate));

if (!mobile && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
  plate.addEventListener("pointermove", (event) => {
    const box = plate.getBoundingClientRect();
    const x = (event.clientX - box.left) / box.width, y = (event.clientY - box.top) / box.height;
    plate.style.setProperty("--rx", `${(0.5 - y) * 9}deg`);
    plate.style.setProperty("--ry", `${(x - 0.5) * 12}deg`);
    surface.aim(x, y);
  });
  plate.addEventListener("pointerleave", () => {
    plate.style.setProperty("--rx", "0deg");
    plate.style.setProperty("--ry", "0deg");
    surface.aim(0.2, 0.15);
  });
}

let state = null;

async function boot(key) {
  show("loading");
  let refused = null;
  if (key) await api("auth", { key }).catch((error) => (refused = error));
  for (;;) {
    try {
      state = await api("state");
      break;
    } catch {
      await sleep(2000);
    }
  }
  $("#version").textContent = state.version;
  route(state.login);
  if (refused && !state.authorized) fail(refused);
}

function route(login) {
  if (!state.authorized) {
    $("#auth-link").textContent = fill(T.auth_link, { command: state.command });
    return show("auth");
  }
  if (!state.can_add) return show("limit");
  if (!state.app) return showApps();
  if (login?.stage && !["cancelled", "failed"].includes(login.stage)) return onLogin(login);
  openLogin(mobile ? "phone" : "qr");
}

// auth

$("#auth-send").addEventListener("click", (event) => busy(event.currentTarget, async () => {
  const wait = $(".wait", screen);
  const ends = Date.now() + 5 * 60 * 1000;
  const tick = () => {
    const left = Math.max(0, Math.round((ends - Date.now()) / 1000));
    $("#countdown").textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
    $(".wait__bar i").style.transform = `scaleX(${left / 300})`;
  };
  tick();
  const timer = setInterval(tick, 1000);
  wait.hidden = false;
  $(".error", screen).textContent = "";
  try {
    await api("auth", {});
    await boot();
  } catch (error) {
    fail(error);
  } finally {
    clearInterval(timer);
    wait.hidden = true;
  }
}));

// app

let preset = null;

function showApps() {
  const list = $("#apps");
  list.replaceChildren(
    ...state.apps.map((app) => choice(app.key, app.title, `${T.official} · API ID ${app.api_id}`)),
    choice("custom", T.own_app, T.own_app_meta),
  );
  pick(preset || state.apps[0]?.key || "custom");
  show("app");
}

function choice(key, title, meta) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "choice";
  button.dataset.key = key;
  button.innerHTML = '<span class="choice__dot"></span><span class="choice__body"><b></b><small></small></span>';
  $("b", button).textContent = title;
  $("small", button).textContent = meta;
  button.addEventListener("click", () => pick(key));
  return button;
}

function pick(key) {
  preset = key;
  $$(".choice").forEach((button) => button.classList.toggle("on", button.dataset.key === key));
  $("#custom").hidden = key !== "custom";
  if (key === "custom") $("#api-id").focus();
}

$("#app-form").addEventListener("submit", (event) => {
  event.preventDefault();
  busy($("button.btn", event.target), async () => {
    const body = preset === "custom"
      ? { api_id: $("#api-id").value.trim(), api_hash: $("#api-hash").value.trim() }
      : { preset };
    try {
      await api("app", body);
      state.app = true;
      openLogin(mobile ? "phone" : "qr");
    } catch (error) {
      fail(error);
    }
  });
});

$("#change-app").addEventListener("click", async () => {
  stopPolling();
  await api("cancel", {}).catch(() => {});
  showApps();
});

// login

let method = null;

function openLogin(name) {
  $("#login-title").textContent = state.setup ? T.setup_title : T.add_title;
  show("login");
  switchMethod(name);
}

async function switchMethod(name) {
  method = name;
  $$("[data-method]").forEach((tab) => tab.setAttribute("aria-selected", tab.dataset.method === name));
  $$("[data-panel]").forEach((panel) => (panel.hidden = panel.dataset.panel !== name));
  $$(".error", screen).forEach((e) => (e.textContent = ""));
  stopPolling();
  if (name === "qr") {
    $("#qr").replaceChildren(Object.assign(document.createElement("div"), { className: "spinner" }));
    try {
      onLogin(await api("qr", {}));
      poll();
    } catch (error) {
      fail(error, $('.error[data-for="qr"]'));
    }
  } else {
    await api("cancel", {}).catch(() => {});
    $("#phone").focus();
  }
}

$$("[data-method]").forEach((tab) => tab.addEventListener("click", () => tab.dataset.method !== method && switchMethod(tab.dataset.method)));

$("#phone-form").addEventListener("submit", (event) => {
  event.preventDefault();
  busy($("button.btn", event.target), async () => {
    try {
      onLogin(await api("phone", { phone: $("#phone").value }));
    } catch (error) {
      fail(error, event.target);
    }
  });
});

let version = -1;
let polling = 0;

function stopPolling() {
  polling++;
}

/** Follows the QR login: the server answers when it changes */
async function poll() {
  const id = ++polling;
  while (id === polling) {
    try {
      const login = await api("login?v=" + version);
      if (id !== polling) break;
      if (login.version !== version) onLogin(login);
      if (login.stage !== "qr") break;
    } catch (error) {
      if (id !== polling) break;
      if (error.code === "auth") return boot();
      await sleep(2000);
    }
  }
}

function onLogin(login) {
  version = login.version;
  switch (login.stage) {
    case "qr":
      if (screen.dataset.screen !== "login") return openLogin("qr");
      $("#qr").innerHTML = login.qr;
      break;
    case "code":
      showCode(login);
      break;
    case "password":
      $("#hint").textContent = login.hint ? fill(T.password_hint, { hint: login.hint }) : "";
      $("#password").value = "";
      show("password");
      break;
    case "done":
      stopPolling();
      $("#done-title").textContent = login.name ? fill(T.done_title, { name: login.name }) : T.done_anon;
      show("done");
      surface.flash();
      plate.classList.add("struck");
      break;
    case "failed":
      fail({ code: login.error }, $('.error[data-for="qr"]'));
      break;
  }
}

// code

let codeLength = 5;

function showCode(login) {
  codeLength = login.length || 5;
  const via = T.code_via[{ firebasesms: "sms", smsword: "sms", smsphrase: "sms", flashcall: "call", missedcall: "call" }[login.via] || login.via];
  $("#code-text").textContent = fill(via || T.code_other, { phone: login.phone });
  const input = $("#code");
  input.value = "";
  input.maxLength = codeLength;
  $("#cells").replaceChildren(...Array.from({ length: codeLength }, () => document.createElement("i")));
  renderCells();
  show("code");
  input.focus();
}

function renderCells() {
  const value = $("#code").value;
  $$("#cells i").forEach((cell, index) => {
    cell.textContent = value[index] || "";
    cell.classList.toggle("now", index === Math.min(value.length, codeLength - 1) && document.activeElement === $("#code"));
  });
}

$("#code").addEventListener("input", (event) => {
  event.target.value = event.target.value.replace(/\D/g, "").slice(0, codeLength);
  renderCells();
  if (event.target.value.length === codeLength) $("#code-form").requestSubmit();
});
["focus", "blur"].forEach((name) => $("#code").addEventListener(name, renderCells));

$("#code-form").addEventListener("submit", (event) => {
  event.preventDefault();
  busy($("button.btn", event.target), async () => {
    try {
      onLogin(await api("code", { code: $("#code").value }));
    } catch (error) {
      fail(error, event.target);
      if (error.code === "code_invalid") {
        $("#code").value = "";
        renderCells();
        $("#code").focus();
      }
      if (error.code === "code_expired") setTimeout(() => openLogin("phone"), 1600);
    }
  });
});

// password

$("#password-form").addEventListener("submit", (event) => {
  event.preventDefault();
  busy($("button.btn", event.target), async () => {
    try {
      onLogin(await api("password", { password: $("#password").value }));
    } catch (error) {
      fail(error, event.target);
      $("#password").select();
    }
  });
});

$$("[data-back]").forEach((button) => button.addEventListener("click", () => openLogin(screen.dataset.screen === "code" ? "phone" : mobile ? "phone" : "qr")));

// .weburl puts a one-time key in the fragment, it shouldn't stay in the address bar or history
const key = new URLSearchParams(location.hash.slice(1)).get("key");
if (key) history.replaceState(null, "", location.pathname + location.search);
boot(key);
