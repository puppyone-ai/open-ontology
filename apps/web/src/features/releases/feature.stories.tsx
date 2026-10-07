import type { Meta, StoryObj } from "@storybook/react-vite";
import { ReleaseDetailPage } from "./pages";

const meta = {
  title: "Admin/ReleaseDetailPage",
  component: ReleaseDetailPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ReleaseDetailPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
