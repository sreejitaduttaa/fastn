window.fastn_utils = {
    htmlNode(kind) {
        let node = "div";
        let css = ["ft_common"];
        if (kind === fastn_dom.ElementKind.Column) {
            css.push("ft_column");
        } else if (kind === fastn_dom.ElementKind.Row) {
            css.push("ft_row");
        } else if (kind === fastn_dom.ElementKind.IFrame) {
            node = "iframe";
        } else if (kind === fastn_dom.ElementKind.Image) {
            node = "img";
        }
        return [node, css];
    },
    getValue(obj) {
        if (!!obj.get) {
           return obj.get();
        } else {
           return obj;
        }
    }
}
