import type { Meta, StoryObj } from "@storybook/react-vite";
import { PermissionPreviewPage } from "./pages";

const meta = {
  title: "Admin/PermissionPreviewPage",
  component: PermissionPreviewPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof PermissionPreviewPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
