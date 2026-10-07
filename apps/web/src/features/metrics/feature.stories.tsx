import type { Meta, StoryObj } from "@storybook/react-vite";
import { MetricDetailPage } from "./pages";

const meta = {
  title: "Admin/MetricDetailPage",
  component: MetricDetailPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof MetricDetailPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
