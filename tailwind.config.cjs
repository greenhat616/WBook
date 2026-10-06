/** @type {import('tailwindcss').Config} */
module.exports = {
  darkMode: ["class"],
  content: [
    "./pages/**/*.{ts,tsx}",
    "./components/**/*.{ts,tsx}",
    "./app/**/*.{ts,tsx}",
    "./src/**/*.{ts,tsx}",
  ],
  theme: {
    container: {
      center: true,
      padding: "2rem",
      screens: {
        "2xl": "1400px",
      },
    },
    extend: {
      // Colors follow the M3 dynamic scheme that <m3e-theme> derives at runtime.
      colors: {
        border: "var(--md-sys-color-outline-variant)",
        input: "var(--md-sys-color-outline)",
        ring: "var(--md-sys-color-primary)",
        background: "var(--md-sys-color-surface)",
        foreground: "var(--md-sys-color-on-surface)",
        primary: {
          DEFAULT: "var(--md-sys-color-primary)",
          foreground: "var(--md-sys-color-on-primary)",
          container: "var(--md-sys-color-primary-container)",
          "on-container": "var(--md-sys-color-on-primary-container)",
        },
        secondary: {
          DEFAULT: "var(--md-sys-color-secondary-container)",
          foreground: "var(--md-sys-color-on-secondary-container)",
        },
        tertiary: {
          DEFAULT: "var(--md-sys-color-tertiary-container)",
          foreground: "var(--md-sys-color-on-tertiary-container)",
        },
        destructive: {
          DEFAULT: "var(--md-sys-color-error)",
          foreground: "var(--md-sys-color-on-error)",
        },
        muted: {
          DEFAULT: "var(--md-sys-color-surface-container-high)",
          foreground: "var(--md-sys-color-on-surface-variant)",
        },
        accent: {
          DEFAULT: "var(--md-sys-color-tertiary-container)",
          foreground: "var(--md-sys-color-on-tertiary-container)",
        },
        popover: {
          DEFAULT: "var(--md-sys-color-surface-container)",
          foreground: "var(--md-sys-color-on-surface)",
        },
        card: {
          DEFAULT: "var(--md-sys-color-surface-container-low)",
          foreground: "var(--md-sys-color-on-surface)",
        },
        surface: {
          DEFAULT: "var(--md-sys-color-surface)",
          container: "var(--md-sys-color-surface-container)",
          "container-high": "var(--md-sys-color-surface-container-high)",
        },
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      keyframes: {
        "accordion-down": {
          from: { height: 0 },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: 0 },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
    },
  },
  plugins: [require("tailwindcss-animate")],
};
