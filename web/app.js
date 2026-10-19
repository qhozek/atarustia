const $ = (id) => document.getElementById(id);
async function api(path, data) {
  const response = await fetch(`/api/${path}`, data === undefined ? {cache:'no-store'} : {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)});
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || 'Ошибка запроса');
  return result;
}
function screen(stage) {
  for (const name of ['setup','connecting','phone','code','password','ready','qr']) $(name).hidden = name !== stage;
}
async function refresh() {
  const state = await api('state');
  const stage = state.login.stage;
  screen(stage === 'done' ? 'ready' : stage);
  $('status').textContent = ['ready','done'].includes(stage) ? 'Аккаунт подключён' : stage === 'setup' ? 'Первый запуск' : 'Вход в Telegram';
  $('hint').textContent = state.login.hint ? `Подсказка: ${state.login.hint}` : '';
  if (state.login.qr) $('qrimage').src = `data:image/svg+xml,${encodeURIComponent(state.login.qr)}`;
  $('accounts').textContent = `Аккаунты: ${state.accounts.join(', ') || 'подключение…'}`;
}
$('preset').addEventListener('change', () => { $('customapp').hidden = $('preset').value !== 'custom'; });
$('setup').addEventListener('submit', async (event) => {
  event.preventDefault(); const button = event.submitter; button.disabled = true; $('error').textContent = '';
  try { await api('setup', {preset:$('preset').value,api_id:$('apiid').value,api_hash:$('apihash').value}); $('apihash').value = ''; $('status').textContent = 'Подключение к Telegram…'; await refresh(); }
  catch(error) { $('error').textContent = error.message; }
  finally { button.disabled = false; }
});
$('byqr').addEventListener('click', async () => { try { await api('qr',{}); await refresh(); } catch(e) { $('error').textContent=e.message; } });
$('byphone').addEventListener('click', () => { screen('phone'); });
setInterval(async () => { if (!$('qr').hidden || !$('setup').hidden || !$('connecting').hidden || !$('ready').hidden) { try { await refresh(); } catch(e) { $('error').textContent=e.message; } } }, 2500);
for (const [name, field, key] of [['phone','number','phone'],['code','digits','code'],['password','secret','password']]) {
  $(name).addEventListener('submit', async (event) => {
    event.preventDefault(); const button = event.submitter;
    if (button.disabled) return; button.disabled = true; $('error').textContent = '';
    const value = $(field).value; if (name !== 'phone') $(field).value = '';
    try { await api(name, {[key]:value}); await refresh(); }
    catch (error) { $('error').textContent = error.message; }
    finally { button.disabled = false; }
  });
}
$('again').addEventListener('click', () => screen('phone'));
const key = new URLSearchParams(location.hash.slice(1)).get('key');
history.replaceState(null,'',location.pathname);
try { if (key) await api('auth',{key}); await refresh(); }
catch (error) { $('status').textContent = 'Нужна новая одноразовая ссылка'; $('error').textContent = error.message; }
