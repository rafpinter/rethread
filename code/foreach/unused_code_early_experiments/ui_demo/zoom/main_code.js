if (window.io) {
  window.socket = io();
  window.socket.emit("code_display");
}

let currentScene;
async function init() {
  document.getElementById("content").innerText = "";

  const c1 = document.getElementById("c1");
  c1.width = window.innerWidth * 2;
  c1.height = window.innerHeight * 2;
  c1.style.zIndex = "0";
  c1.style.width = "100%";
  c1.style.height = "100%";
  c1.style.left = "0";

  const bg = document.getElementById("bg");
  bg.style.opacity = "0";

  // const captureScene = new CaptureScene(c1, null);
  // const img = await loadScene(captureScene);

  // const zoomTransitionScene = new ZoomTransitionScene(c1, img, null);
  // const rgbTransitionScene = new RGBScene(c1, img, null);
  // await loadScene(Math.random() < 0 ? rgbTransitionScene : zoomTransitionScene);

  const backupImg = await loadImage("./img/portrait.jpg");
  const codeScene = new CodeScene(c1, backupImg);
  await loadScene(codeScene);
}

async function loadScene(scene) {
  if (currentScene) await currentScene.unload();
  await scene.init();
  currentScene = scene;
  document.getElementById("content").className =
    "filter-" + scene.constructor.name;

  if (scene.cb !== undefined) {
    return new Promise((resolve) => {
      scene.cb = resolve;
    });
  }
}

async function animate() {
  window.requestAnimationFrame(animate);

  if (currentScene) currentScene.render();
}

window.addEventListener("keydown", (e) => {
  if (e.key == "i") {
    init();
  }
});

if (window.socket)
  window.socket.on("capture", () => {
    if (currentScene.name != "capture") {
      init();
    }
  });

// initBg();
