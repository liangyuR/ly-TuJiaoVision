import { Component, type ReactNode } from "react";

/** 页面渲染出错时只把这一页换成报错，侧栏和别的页面照常能用。 */
export default class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="panel">
        <h3 className="panel-title">页面出错了</h3>
        <p className="muted mono">{this.state.error.message}</p>
        <button className="btn" onClick={() => this.setState({ error: null })}>
          重试
        </button>
      </div>
    );
  }
}
