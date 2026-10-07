import type { Meta, StoryObj } from "@storybook/react-vite";
import { SourcesPage } from "./pages";

const meta = {
  title: "Admin/SourcesPage",
  component: SourcesPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof SourcesPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
