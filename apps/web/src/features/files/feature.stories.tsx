import type { Meta, StoryObj } from "@storybook/react-vite";
import { FileWizardPage } from "./pages";

const meta = {
  title: "Admin/FileWizardPage",
  component: FileWizardPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof FileWizardPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
