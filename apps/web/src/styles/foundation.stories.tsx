import type { Meta, StoryObj } from "@storybook/react-vite";
import { Foundation } from "./foundation";

const meta = {
  title: "Foundation/Tokens",
  component: Foundation,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof Foundation>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
