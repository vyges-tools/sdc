module t(clk,a,q);
input clk; input [2:0] a; output [2:0] q;
wire \u.t[0] , \u.t_reg[0] , \v.s[0] ;
DFF u1(.CK(clk),.D(a[0]),.Q(\u.t[0] ));
DFF u2(.CK(clk),.D(a[1]),.Q(\u.t_reg[0] ));
DFF u3(.CK(clk),.D(a[2]),.Q(\v.s[0] ));
INV u4(.A(\u.t[0] ),.Y(q[0]));
INV u5(.A(\u.t_reg[0] ),.Y(q[1]));
INV u6(.A(\v.s[0] ),.Y(q[2]));
endmodule
