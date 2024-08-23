const audio = new Audio("assets/One by Metallica.mp3");
audio.volume = 0.5;
audio.loop = true;

document.getElementById("play-button").addEventListener("click", function () {
  audio
    .play()
    .then(() => {
      document.getElementById("play-button").style.display = "none";
      document.getElementById("now-playing").style.display = "inline-block";
    })
    .catch((error) => {
      console.error("Error playing audio:", error);
      alert(
        "Failed to play audio. Please check the console for more information."
      );
    });
});

audio.addEventListener("error", function () {
  console.error("Audio error:", audio.error);
  console.log("Audio URL:", audio.src);
  alert("Failed to load audio. Please check the console for more information.");
});

// Matrix background script
const canvas = document.getElementById("matrixCanvas");
const ctx = canvas.getContext("2d");
canvas.width = window.innerWidth;
canvas.height = window.innerHeight;

const fontSize = 16;
const columns = canvas.width / fontSize;
const drops = Array(Math.floor(columns)).fill(1);

const characters = "アカサタナハマヤラワガザダバパヒフヘホヲン";

function drawMatrix() {
  ctx.fillStyle = "rgba(0, 0, 0, 0.15)";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = "#300f0f";
  ctx.font = fontSize + "px monospace";

  for (let i = 0; i < drops.length; i++) {
    const text = characters[Math.floor(Math.random() * characters.length)];
    ctx.fillText(text, i * fontSize, drops[i] * fontSize);

    if (drops[i] * fontSize > canvas.height && Math.random() > 0.975) {
      drops[i] = 0;
    }
    drops[i]++;
  }
}

setInterval(drawMatrix, 35);

window.addEventListener("resize", () => {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
  drops.length = Math.floor(canvas.width / fontSize);
  drops.fill(1);
});
