import type { Meta, StoryObj } from "@storybook/react-vite";
import { BuildDetailPage } from "./pages";

const meta = {
  title: "Admin/BuildDetailPage",
  component: BuildDetailPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof BuildDetailPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
