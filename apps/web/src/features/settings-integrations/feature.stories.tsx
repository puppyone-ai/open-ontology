import type { Meta, StoryObj } from "@storybook/react-vite";
import { ChannelsPage } from "./pages";

const meta = {
  title: "Admin/ChannelsPage",
  component: ChannelsPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ChannelsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
