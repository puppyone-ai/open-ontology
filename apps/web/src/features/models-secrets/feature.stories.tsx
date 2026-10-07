import type { Meta, StoryObj } from "@storybook/react-vite";
import { SecretsPage } from "./pages";

const meta = {
  title: "Admin/SecretsPage",
  component: SecretsPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof SecretsPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
