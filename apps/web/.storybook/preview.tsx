import { useEffect } from "react";
import type { Decorator, Preview } from "@storybook/react-vite";
import { withThemeByDataAttribute } from "@storybook/addon-themes";
import "../src/styles/app.css";

const withDensity: Decorator = (Story, context) => {
  const density = typeof context.globals.density === "string" ? context.globals.density : "comfortable";
  useEffect(() => {
    document.documentElement.dataset.density = density;
  }, [density]);
  return <Story />;
};

const preview: Preview = {
  decorators: [
    withThemeByDataAttribute({
      themes: { light: "light", dark: "dark" },
      defaultTheme: "light",
      attributeName: "data-theme",
    }),
    withDensity,
  ],
  globalTypes: {
    density: {
      description: "行高",
      toolbar: {
        icon: "component",
        items: [
          { value: "compact", title: "紧凑" },
          { value: "comfortable", title: "舒适" },
          { value: "roomy", title: "宽松" },
        ],
      },
    },
  },
  initialGlobals: { density: "comfortable" },
  parameters: { layout: "fullscreen" },
};

export default preview;
