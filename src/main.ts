import { createApp } from "vue";
import App from "./App.vue";

const FUNCTION_KEY_PATTERN = /^F([1-9]|1[0-2])$/;

function shouldBlockKey(event: KeyboardEvent) {
  if (FUNCTION_KEY_PATTERN.test(event.key)) return true;
  if (event.key === "ContextMenu") return true;

  const key = event.key.toLowerCase();
  const withCtrlOrMeta = event.ctrlKey || event.metaKey;

  return (
    key === "browserback" ||
    key === "browserforward" ||
    (withCtrlOrMeta && key === "r") ||
    (withCtrlOrMeta && key === "u") ||
    (withCtrlOrMeta && event.shiftKey && ["c", "i", "j"].includes(key))
  );
}

window.addEventListener(
  "contextmenu",
  (event) => {
    event.preventDefault();
  },
  { capture: true },
);

window.addEventListener(
  "keydown",
  (event) => {
    if (!shouldBlockKey(event)) return;
    event.preventDefault();
    event.stopPropagation();
  },
  { capture: true },
);

createApp(App).mount("#app");
