const columns = ["时间", "工件编号", "配方", "结果", "耗时 (ms)"];

export default function HistoryPage() {
  return (
    <div className="panel">
      <table className="table">
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c}>{c}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          <tr>
            <td colSpan={columns.length} className="muted center">
              暂无记录
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}
