/** @type {import('tailwindcss').Config} */
export default {
  darkMode: "class",
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Neutral surface ladder (page → card → raised → border). Values follow
        // the flat, low-contrast scale used by desktop mod managers so the acid
        // accent stays the only strong colour on screen.
        ink: {
          950: "#0f0f0f",
          900: "#1a1a1a",
          850: "#141414",
          800: "#242424",
          700: "#2d2d2d",
          600: "#3a3a3a",
          500: "#52525b",
        },
        // ZZMM's primary signal colour (acid yellow-green).
        acid: {
          200: "#f0ffb0",
          300: "#e4ff7c",
          400: "#d2ff3c",
          500: "#b5ec0d",
          600: "#8fc000",
        },
        // Secondary "urban" accent, reserved for destructive/conflict states.
        hot: {
          300: "#ff8ab4",
          400: "#ff4d8d",
          500: "#ff1f6f",
          600: "#d90a53",
        },
      },
      fontFamily: {
        display: [
          "Bahnschrift",
          "Segoe UI Variable Display",
          "Segoe UI",
          "system-ui",
          "sans-serif",
        ],
        sans: [
          "Segoe UI Variable Text",
          "Segoe UI",
          "system-ui",
          "sans-serif",
        ],
        mono: ["Cascadia Mono", "Cascadia Code", "Consolas", "monospace"],
      },
      transitionTimingFunction: {
        // The eased curve used for every hover/panel transition.
        smooth: "cubic-bezier(0.22, 1, 0.36, 1)",
      },
      boxShadow: {
        card: "0 1px 2px rgba(0, 0, 0, 0.45)",
        pop: "0 18px 40px -18px rgba(0, 0, 0, 0.85)",
        neon: "0 0 0 1px rgba(210, 255, 60, 0.35), 0 0 30px -8px rgba(210, 255, 60, 0.45)",
      },
      keyframes: {
        "toast-in": {
          "0%": { opacity: "0", transform: "translateY(8px) scale(0.98)" },
          "100%": { opacity: "1", transform: "translateY(0) scale(1)" },
        },
        "fade-in": {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" },
        },
        "panel-in": {
          "0%": { opacity: "0", transform: "translateX(16px)" },
          "100%": { opacity: "1", transform: "translateX(0)" },
        },
      },
      animation: {
        "toast-in": "toast-in 180ms cubic-bezier(0.22, 1, 0.36, 1)",
        "fade-in": "fade-in 200ms cubic-bezier(0.22, 1, 0.36, 1)",
        "panel-in": "panel-in 220ms cubic-bezier(0.22, 1, 0.36, 1)",
      },
    },
  },
  plugins: [],
};
