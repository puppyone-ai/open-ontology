import type { Meta, StoryObj } from "@storybook/react-vite";
import { RecoveryPage } from "./pages";

const meta = {
  title: "Admin/RecoveryPage",
  component: RecoveryPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof RecoveryPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
