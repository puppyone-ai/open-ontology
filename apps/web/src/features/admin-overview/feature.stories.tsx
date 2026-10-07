import type { Meta, StoryObj } from "@storybook/react-vite";
import { OverviewPage } from "./pages";

const meta = {
  title: "Admin/OverviewPage",
  component: OverviewPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof OverviewPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
