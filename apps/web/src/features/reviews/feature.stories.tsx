import type { Meta, StoryObj } from "@storybook/react-vite";
import { ReviewDetailPage } from "./pages";

const meta = {
  title: "Admin/ReviewDetailPage",
  component: ReviewDetailPage,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof ReviewDetailPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NotConnected: Story = {};
