import type { Meta, StoryObj } from "@storybook/react-vite";
import { ClassificationsPage } from "./pages";

const meta = {
  title: "Admin/ClassificationsPage",
  component: ClassificationsPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ClassificationsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
