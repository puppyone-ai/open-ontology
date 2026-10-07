import type { Meta, StoryObj } from "@storybook/react-vite";
import { JobsPage } from "./pages";

const meta = {
  title: "Admin/JobsPage",
  component: JobsPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof JobsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
