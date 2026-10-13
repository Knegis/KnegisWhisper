/** Shared browser interactions for fixture checks. No application state is changed here. */
import { centerOfText } from "./snippets.mjs";
export const delay = ms => new Promise(done => setTimeout(done, ms));
export async function waitFor(cdp, expression, description = expression, timeout = 15000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    try { if (await cdp.evaluate(expression)) return; } catch { /* A reload can remove its execution context. */ }
    await delay(60);
  }
  throw Error(`Timed out waiting for ${description}`);
}
export async function reload(cdp, timeout = 15000) {
  let stop = () => {}; let timer;
  const loaded = new Promise((done, reject) => {
    timer = setTimeout(() => reject(Error("Timed out waiting for reload")), timeout);
    stop = cdp.on("Page.loadEventFired", done);
  });
  try { await cdp.send("Page.reload", { ignoreCache: true }); await loaded; }
  finally { clearTimeout(timer); stop(); }
}
/** A real pointer press at `point`; `modifiers` is CDP's bit field (Alt 1, Ctrl 2, Meta 4, Shift 8). */
export async function press(cdp, point, { modifiers = 0 } = {}) {
  const { x, y } = point;
  await cdp.send("Input.dispatchMouseEvent", {type:"mouseMoved",x,y,modifiers});
  await cdp.send("Input.dispatchMouseEvent", {type:"mousePressed",x,y,button:"left",clickCount:1,modifiers});
  await cdp.send("Input.dispatchMouseEvent", {type:"mouseReleased",x,y,button:"left",clickCount:1,modifiers});
  await delay(150);
}
export async function pressText(cdp, selector, text) {
  await cdp.evaluate(`[...document.querySelectorAll(${JSON.stringify(selector)})].find(item => ${text === null ? "true" : `item.textContent.trim() === ${JSON.stringify(text)}`})?.scrollIntoView({block:"center"})`);
  const point = await cdp.evaluate(centerOfText(selector, text));
  const size = await cdp.evaluate("({width:innerWidth,height:innerHeight})");
  if (point.x < 0 || point.y < 0 || point.x > size.width || point.y > size.height) throw Error(`${selector} ${text} is off screen`);
  await press(cdp, point);
}
