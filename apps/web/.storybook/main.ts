import type { StorybookConfig } from "@storybook/react-vite";

const config: StorybookConfig = {
  stories: [
    "../src/styles/**/*.stories.tsx",
    "../src/components/**/*.stories.tsx",
    "../src/features/**/*.stories.tsx",
  ],
  addons: ["@storybook/addon-a11y", "@storybook/addon-themes"],
  framework: "@storybook/react-vite",
};

export default config;
